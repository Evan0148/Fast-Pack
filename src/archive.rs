use std::io::{self, Seek, SeekFrom, Write};

const CAPACITY: usize = 256 * 1024;

pub(crate) struct ArchiveWriter<W> {
    inner: W,
    buffer: Vec<u8>,
    start: u64,
    cursor: usize,
}

impl<W: Write + Seek> ArchiveWriter<W> {
    pub(crate) fn new(mut inner: W) -> io::Result<Self> {
        let start = inner.stream_position()?;
        Ok(Self {
            inner,
            buffer: Vec::with_capacity(CAPACITY),
            start,
            cursor: 0,
        })
    }

    fn commit(&mut self) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let position = self.start + self.cursor as u64;
        self.inner.seek(SeekFrom::Start(self.start))?;
        self.inner.write_all(&self.buffer)?;
        if self.cursor != self.buffer.len() {
            self.inner.seek(SeekFrom::Start(position))?;
        }
        self.start = position;
        self.cursor = 0;
        self.buffer.clear();
        Ok(())
    }
}

impl<W: Write + Seek> Write for ArchiveWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.cursor == CAPACITY {
            self.commit()?;
        }
        let count = bytes.len().min(CAPACITY - self.cursor);
        let end = self.cursor + count;
        if end > self.buffer.len() {
            self.buffer.resize(end, 0);
        }
        self.buffer[self.cursor..end].copy_from_slice(&bytes[..count]);
        self.cursor = end;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.commit()?;
        self.inner.flush()
    }
}

impl<W: Write + Seek> Seek for ArchiveWriter<W> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let position = match from {
            SeekFrom::Start(position) => position,
            SeekFrom::Current(offset) => (self.start + self.cursor as u64)
                .checked_add_signed(offset)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "ZIP 写入位置无效"))?,
            SeekFrom::End(offset) => {
                self.commit()?;
                self.start = self.inner.seek(SeekFrom::End(offset))?;
                return Ok(self.start);
            }
        };
        if position >= self.start && position - self.start <= self.buffer.len() as u64 {
            self.cursor = (position - self.start) as usize;
        } else {
            self.commit()?;
            self.start = self.inner.seek(SeekFrom::Start(position))?;
        }
        Ok(position)
    }
}
