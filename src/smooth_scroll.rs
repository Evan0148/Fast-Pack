//! Interpolate discrete wheel notches inside the widget, without rebuilding App::view.
use super::Message;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
    widget::{Id, Operation, Tree, operation, tree},
};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, window};
use std::time::Instant;

#[derive(Default)]
struct Motion {
    target: Option<f32>,
    position: Option<f32>,
    previous: Option<Instant>,
}

impl Motion {
    fn cancel(&mut self) {
        self.target = None;
        self.position = None;
        self.previous = None;
    }
    fn push(&mut self, current: f32, delta: f32, limit: f32, now: Instant) {
        self.target = Some((self.target.unwrap_or(current) + delta).clamp(0.0, limit));
        self.position.get_or_insert(current);
        self.previous.get_or_insert(now);
    }
    fn advance(&mut self, current: f32, limit: f32, now: Instant) -> f32 {
        let Some(target) = self.target.map(|target| target.clamp(0.0, limit)) else {
            return current;
        };
        let elapsed = now
            .saturating_duration_since(self.previous.replace(now).unwrap_or(now))
            .as_secs_f32()
            .min(0.05);
        // Scrollable exposes rounded drawing coordinates. Keep fractional progress
        // here so a high refresh rate cannot stall one pixel short of the target.
        let current = self.position.unwrap_or(current).clamp(0.0, limit);
        let next = current + (target - current) * (1.0 - (-elapsed / 0.024).exp());
        if (target - next).abs() <= 0.15 {
            self.cancel();
            target
        } else {
            self.target = Some(target);
            self.position = Some(next);
            next
        }
    }
}

#[derive(Clone, Copy)]
struct Position {
    offset: f32,
    limit: f32,
}

struct TrackScroll<'a> {
    state: &'a mut dyn operation::Scrollable,
    changed: &'a mut bool,
}
impl operation::Scrollable for TrackScroll<'_> {
    fn snap_to(&mut self, offset: operation::scrollable::RelativeOffset<Option<f32>>) {
        *self.changed = true;
        self.state.snap_to(offset);
    }
    fn scroll_to(&mut self, offset: operation::scrollable::AbsoluteOffset<Option<f32>>) {
        *self.changed = true;
        self.state.scroll_to(offset);
    }
    fn scroll_by(
        &mut self,
        offset: operation::scrollable::AbsoluteOffset,
        bounds: Rectangle,
        content: Rectangle,
    ) {
        *self.changed = true;
        self.state.scroll_by(offset, bounds, content);
    }
}

struct TrackOperation<'a> {
    operation: &'a mut dyn Operation,
    id: &'a Id,
    changed: &'a mut bool,
}
impl Operation for TrackOperation<'_> {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        let id = self.id;
        let changed = &mut *self.changed;
        self.operation.traverse(&mut |operation| {
            operate(&mut TrackOperation {
                operation,
                id,
                changed,
            })
        });
    }
    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        self.operation.container(id, bounds);
    }
    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        state: &mut dyn operation::Scrollable,
    ) {
        if id == Some(self.id) {
            self.operation.scrollable(
                id,
                bounds,
                content,
                translation,
                &mut TrackScroll {
                    state,
                    changed: self.changed,
                },
            );
        } else {
            self.operation
                .scrollable(id, bounds, content, translation, state);
        }
    }
    fn focusable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        state: &mut dyn operation::Focusable,
    ) {
        self.operation.focusable(id, bounds, state);
    }
    fn text_input(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        state: &mut dyn operation::TextInput,
    ) {
        self.operation.text_input(id, bounds, state);
    }
    fn text(&mut self, id: Option<&Id>, bounds: Rectangle, text: &str) {
        self.operation.text(id, bounds, text);
    }
    fn custom(&mut self, id: Option<&Id>, bounds: Rectangle, state: &mut dyn std::any::Any) {
        self.operation.custom(id, bounds, state);
    }
}

struct Offset<'a> {
    id: &'a Id,
    position: Option<Position>,
    set: Option<f32>,
}
impl Operation for Offset<'_> {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        if self.position.is_none() {
            operate(self);
        }
    }
    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        state: &mut dyn operation::Scrollable,
    ) {
        if id == Some(self.id) {
            self.position = Some(Position {
                offset: translation.y,
                limit: (content.height - bounds.height).max(0.0),
            });
            if let Some(offset) = self.set {
                state.scroll_to(operation::scrollable::AbsoluteOffset {
                    x: None,
                    y: Some(offset),
                });
            }
        }
    }
}

struct Smooth<'a> {
    content: Element<'a, Message>,
    id: Id,
    reduced: bool,
}

pub(super) fn scroll<'a>(
    content: impl Into<Element<'a, Message>>,
    id: &'static str,
    reduced: bool,
) -> Element<'a, Message> {
    Element::new(Smooth {
        content: content.into(),
        id: Id::new(id),
        reduced,
    })
}

impl Smooth<'_> {
    fn offset(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        set: Option<f32>,
    ) -> Option<Position> {
        let mut operation = Offset {
            id: &self.id,
            position: None,
            set,
        };
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, &mut operation);
        operation.position
    }
}

impl Widget<Message, Theme, Renderer> for Smooth<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Motion>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(Motion::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.content.as_widget())]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.content]);
        if self.reduced {
            tree.state.downcast_mut::<Motion>().cancel();
        }
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let motion = tree.state.downcast_mut::<Motion>();
        let child = &mut tree.children[0];
        if self.reduced {
            motion.cancel();
        }
        if let Event::Window(window::Event::RedrawRequested(now)) = event
            && motion.target.is_some()
            && let Some(position) = self.offset(child, layout, renderer, None)
        {
            let next = motion.advance(position.offset, position.limit, *now);
            self.offset(child, layout, renderer, Some(next));
            if motion.target.is_some() {
                shell.request_redraw();
            }
        }
        let wheel = matches!(
            event,
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { .. }
            })
        ) && !self.reduced
            && !shell.is_event_captured();
        let before = wheel
            .then(|| self.offset(child, layout, renderer, None))
            .flatten();
        self.content.as_widget_mut().update(
            child, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
        if let Some(before) = before {
            if let Some(after) = self.offset(child, layout, renderer, None) {
                let delta = after.offset - before.offset;
                if delta.abs() > 0.01 {
                    // The child gets first refusal: an inner list or editor consuming the
                    // wheel leaves this scrollable unchanged and must never move the page.
                    let distance = if let Event::Mouse(mouse::Event::WheelScrolled {
                        delta: mouse::ScrollDelta::Lines { y, .. },
                    }) = event
                    {
                        if y.abs() > 0.0 && (-y).signum() == delta.signum() {
                            -y * 60.0
                        } else {
                            delta
                        }
                    } else {
                        delta
                    };
                    motion.push(before.offset, distance, after.limit, Instant::now());
                    self.offset(child, layout, renderer, Some(before.offset));
                    shell.request_redraw();
                } else {
                    motion.cancel();
                }
            }
        } else if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(_) | mouse::Event::WheelScrolled { .. })
                | Event::Touch(_)
                | Event::Keyboard(_)
                | Event::Window(window::Event::Unfocused)
        ) {
            motion.cancel();
        }
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let mut changed = false;
        let mut operation = TrackOperation {
            operation,
            id: &self.id,
            changed: &mut changed,
        };
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout,
            renderer,
            &mut operation,
        );
        if changed {
            tree.state.downcast_mut::<Motion>().cancel();
        }
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
#[path = "smooth_scroll_tests.rs"]
mod tests;
