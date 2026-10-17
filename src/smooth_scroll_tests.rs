use super::*;
#[cfg(not(feature = "gpu-renderer"))]
use iced::widget::{column, scrollable, space};
use std::time::Duration;

#[test]
fn wheel_motion_is_continuous_accumulates_and_settles_exactly() {
    let start = Instant::now();
    let mut motion = Motion::default();
    motion.push(0.0, 60.0, 500.0, start);
    let first = motion.advance(0.0, 500.0, start + Duration::from_millis(16));
    assert!((0.0..60.0).contains(&first));
    motion.push(first, 60.0, 500.0, start + Duration::from_millis(16));
    assert_eq!(motion.target, Some(120.0));
    let mut value = first;
    for frame in 2..25 {
        let next = motion.advance(value, 500.0, start + Duration::from_millis(frame * 8));
        assert!(next >= value && next <= 120.0);
        value = next;
    }
    assert_eq!(value, 120.0);
    assert!(motion.target.is_none());
    motion.push(value, -600.0, 500.0, start + Duration::from_millis(220));
    assert_eq!(motion.target, Some(0.0));
    assert!(motion.advance(value, 50.0, start + Duration::from_millis(240)) < value);
    let mut motion = Motion::default();
    motion.push(200.0, 240.0, 1000.0, start);
    let current = motion.advance(200.0, 1000.0, start + Duration::from_millis(16));
    motion.push(current, -60.0, 1000.0, start + Duration::from_millis(16));
    assert!(
        motion.target.unwrap() < current,
        "reversing the wheel must discard the previous direction's backlog"
    );
    assert!(motion.advance(current, 1000.0, start + Duration::from_millis(24)) < current);
}

#[cfg(not(feature = "gpu-renderer"))]
#[test]
fn real_widget_smooths_only_wheel_lines_without_messages_or_nested_scroll_leaks() {
    fn element(reduced: bool) -> Element<'static, Message> {
        let inner = scroll(
            scrollable(space().height(800))
                .id("inner")
                .height(100)
                .width(400),
            "inner",
            reduced,
        );
        scroll(
            scrollable(column![inner, space().height(1000)])
                .id("outer")
                .height(300)
                .width(400),
            "outer",
            reduced,
        )
    }
    fn position(
        view: &mut Element<'_, Message>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &Renderer,
        id: &'static str,
    ) -> f32 {
        let mut operation = Offset {
            id: &Id::new(id),
            position: None,
            set: None,
        };
        view.as_widget_mut()
            .operate(tree, Layout::new(node), renderer, &mut operation);
        operation.position.unwrap().offset
    }
    fn update(
        view: &mut Element<'_, Message>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &Renderer,
        event: Event,
        cursor: iced::Point,
    ) -> Vec<Message> {
        let mut messages = Vec::new();
        view.as_widget_mut().update(
            tree,
            &event,
            Layout::new(node),
            mouse::Cursor::Available(cursor),
            renderer,
            &mut iced::advanced::clipboard::Null,
            &mut Shell::new(&mut messages),
            &Rectangle::with_size(Size::new(400.0, 300.0)),
        );
        messages
    }
    let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
    for reduced in [false, true] {
        let mut view = element(reduced);
        let mut tree = Tree::new(view.as_widget());
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(400.0, 300.0)),
        );
        let cursor = iced::Point::new(150.0, 200.0);
        let event = Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
        });
        assert!(
            update(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                event.clone(),
                cursor
            )
            .is_empty()
        );
        let first = position(&mut view, &mut tree, &node, &renderer, "outer");
        if reduced {
            assert_eq!(first, 60.0);
            continue;
        }
        assert_eq!(
            first, 0.0,
            "the first wheel notch must not jump a whole 60px"
        );
        let now = Instant::now();
        let mut last = first;
        for frame in 1..=24 {
            let event = Event::Window(window::Event::RedrawRequested(
                now + Duration::from_millis(frame * 8),
            ));
            assert!(update(&mut view, &mut tree, &node, &renderer, event, cursor).is_empty());
            let current = position(&mut view, &mut tree, &node, &renderer, "outer");
            assert!(current >= last && current <= 60.0);
            last = current;
        }
        assert_eq!(last, 60.0);
        let event = Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -23.0 },
        });
        update(&mut view, &mut tree, &node, &renderer, event, cursor);
        assert_eq!(
            position(&mut view, &mut tree, &node, &renderer, "outer"),
            83.0,
            "touchpad stays direct"
        );
        // External scroll-to and scrollbar dragging must override all pending animation.
        update(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
            }),
            cursor,
        );
        view.as_widget_mut().operate(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &mut operation::scrollable::scroll_to::<()>(
                Id::new("outer"),
                operation::scrollable::AbsoluteOffset {
                    x: None,
                    y: Some(0.0),
                },
            ),
        );
        let cursor = iced::Point::new(150.0, 40.0);
        update(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(
                Instant::now() + Duration::from_millis(32),
            )),
            cursor,
        );
        assert_eq!(
            position(&mut view, &mut tree, &node, &renderer, "outer"),
            0.0
        );
        // A completed pointer interaction also ends Iced's native scroll transaction.
        update(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            cursor,
        );
        update(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
            }),
            cursor,
        );
        for frame in 1..=24 {
            update(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                Event::Window(window::Event::RedrawRequested(
                    Instant::now() + Duration::from_millis(frame * 8),
                )),
                cursor,
            );
        }
        assert_eq!(
            position(&mut view, &mut tree, &node, &renderer, "outer"),
            0.0,
            "scrolling an inner list must not move the outer page"
        );
        assert_eq!(
            position(&mut view, &mut tree, &node, &renderer, "inner"),
            60.0
        );
    }
}
