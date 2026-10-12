fn main() {
    if let Err(error) = fast_pack::cli::run() {
        eprintln!("Fast Pack: {error:#}");
        std::process::exit(1);
    }
}
