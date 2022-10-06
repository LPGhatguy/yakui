use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use glam::Vec2;
use smallvec::SmallVec;

use crate::dom::{Dom, DomNode};
use crate::event::{Event, EventInterest, EventResponse, WidgetEvent};
use crate::id::WidgetId;
use crate::layout::LayoutDom;
use crate::widget::EventContext;

use super::mouse::MouseButton;
use super::{KeyCode, Modifiers};

/// Holds yakui's input state, like cursor position, hovered, and selected
/// widgets.
#[derive(Debug)]
pub struct InputState {
    /// State for the mouse, like buttons and position.
    mouse: RefCell<Mouse>,

    /// State of the keyboard modifier keys
    modifiers: Cell<Modifiers>,

    /// Details about widgets and their mouse intersections.
    intersections: RefCell<Intersections>,

    /// The widget that is currently selected.
    selection: Cell<Option<WidgetId>>,

    /// The widget that was selected last frame.
    last_selection: Cell<Option<WidgetId>>,
}

#[derive(Debug)]
struct Mouse {
    /// The current mouse position, or `None` if it's outside the window.
    position: Option<Vec2>,

    /// The state of each mouse button. If missing from the map, the button is
    /// up and has not yet been pressed.
    buttons: HashMap<MouseButton, ButtonState>,
}

#[derive(Debug)]
struct Intersections {
    /// All of the widgets with mouse interest that the current mouse position
    /// intersects with.
    ///
    /// All lists like this are stored in reverse depth first order.
    mouse_hit: Vec<WidgetId>,

    /// All of the widgets that have had a mouse enter event sent to them
    /// without a corresponding mouse leave event yet. This is different from
    /// mouse_hit because hover events can be sunk by event handlers.
    mouse_entered: Vec<WidgetId>,

    /// All of the widgets that had a mouse enter event sent to them and then
    /// sunk it that are still being hovered. This helps us ensure that a widget
    /// that sunk a hover event will continue to occupy that space even if we
    /// don't send it more events.
    mouse_entered_and_sunk: Vec<WidgetId>,

    /// All widgets that had the corresponding mouse button pressed while the
    /// mouse cursor was over them.
    #[allow(unused)]
    mouse_down_in: HashMap<MouseButton, Vec<WidgetId>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ButtonState {
    JustDown,
    Down,
    JustUp,
    Up,
}

impl ButtonState {
    pub fn is_down(&self) -> bool {
        matches!(self, Self::JustDown | Self::Down)
    }

    pub fn settle(&mut self) {
        match self {
            Self::JustDown => {
                *self = Self::Down;
            }
            Self::JustUp => {
                *self = Self::Up;
            }
            _ => (),
        }
    }
}

impl InputState {
    /// Create a new, empty `InputState`.
    pub fn new() -> Self {
        Self {
            mouse: RefCell::new(Mouse {
                position: None,
                buttons: HashMap::new(),
            }),
            modifiers: Cell::new(Modifiers::default()),
            intersections: RefCell::new(Intersections {
                mouse_hit: Vec::new(),
                mouse_entered: Vec::new(),
                mouse_entered_and_sunk: Vec::new(),
                mouse_down_in: HashMap::new(),
            }),
            last_selection: Cell::new(None),
            selection: Cell::new(None),
        }
    }

    /// Begin a new frame for input handling.
    pub fn start(&self, dom: &Dom, layout: &LayoutDom) {
        self.notify_selection(dom, layout);
    }

    /// Finish applying input events for this frame.
    pub fn finish(&self) {
        self.settle_buttons();
    }

    /// Return the currently selected widget, if there is one.
    pub fn selection(&self) -> Option<WidgetId> {
        self.selection.get()
    }

    /// Set the currently selected widget.
    pub fn set_selection(&self, id: Option<WidgetId>) {
        self.selection.set(id);
    }

    pub(crate) fn handle_event(
        &self,
        dom: &Dom,
        layout: &LayoutDom,
        event: &Event,
    ) -> EventResponse {
        match event {
            Event::CursorMoved(pos) => {
                self.mouse_moved(dom, layout, *pos);
                EventResponse::Bubble
            }
            Event::MouseButtonChanged { button, down } => {
                self.mouse_button_changed(dom, layout, *button, *down)
            }
            Event::MouseScroll { delta } => self.send_mouse_scroll(dom, layout, *delta),
            Event::KeyChanged { key, down } => self.keyboard_key_changed(dom, layout, *key, *down),
            Event::ModifiersChanged(modifiers) => self.modifiers_changed(modifiers),
            Event::TextInput(c) => self.text_input(dom, layout, *c),
            _ => EventResponse::Bubble,
        }
    }

    fn notify_selection(&self, dom: &Dom, layout: &LayoutDom) {
        let current = self.selection.get();
        let last = self.last_selection.get();

        if current == last {
            return;
        }

        if let Some(entered) = current {
            let mut node = dom.get_mut(entered).unwrap();
            self.fire_event(
                dom,
                layout,
                entered,
                &mut node,
                &WidgetEvent::FocusChanged(true),
            );
        }

        if let Some(left) = last {
            let mut node = dom.get_mut(left).unwrap();
            self.fire_event(
                dom,
                layout,
                left,
                &mut node,
                &WidgetEvent::FocusChanged(false),
            );
        }

        self.last_selection.set(current);
    }

    /// Signal that the mouse has moved.
    fn mouse_moved(&self, dom: &Dom, layout: &LayoutDom, pos: Option<Vec2>) {
        let pos = pos.map(|pos| pos - layout.unscaled_viewport().pos());

        {
            let mut mouse = self.mouse.borrow_mut();
            mouse.position = pos;
        }

        self.send_mouse_move(dom, layout);
        self.mouse_hit_test(dom, layout);
        self.send_mouse_enter(dom, layout);
        self.send_mouse_leave(dom, layout);
    }

    /// Signal that a mouse button's state has changed.
    fn mouse_button_changed(
        &self,
        dom: &Dom,
        layout: &LayoutDom,
        button: MouseButton,
        down: bool,
    ) -> EventResponse {
        {
            let mut mouse = self.mouse.borrow_mut();
            let state = mouse.buttons.entry(button).or_insert(ButtonState::Up);

            match (state.is_down(), down) {
                // If the state didn't actually change, leave the current value
                // alone.
                (true, true) | (false, false) => (),

                (false, true) => {
                    *state = ButtonState::JustDown;
                }

                (true, false) => {
                    *state = ButtonState::JustUp;
                }
            }
        }

        self.send_button_change(dom, layout, button, down)
    }

    fn keyboard_key_changed(
        &self,
        dom: &Dom,
        layout: &LayoutDom,
        key: KeyCode,
        down: bool,
    ) -> EventResponse {
        let selected = self.selection.get();
        if let Some(id) = selected {
            let layout_node = layout.get(id).unwrap();

            if layout_node
                .event_interest
                .contains(EventInterest::FOCUSED_KEYBOARD)
            {
                let mut node = dom.get_mut(id).unwrap();
                let event = WidgetEvent::KeyChanged {
                    key,
                    down,
                    modifiers: self.modifiers.get(),
                };
                return self.fire_event(dom, layout, id, &mut node, &event);
            }
        }

        EventResponse::Bubble
    }

    fn modifiers_changed(&self, modifiers: &Modifiers) -> EventResponse {
        self.modifiers.set(*modifiers);
        EventResponse::Bubble
    }

    fn text_input(&self, dom: &Dom, layout: &LayoutDom, c: char) -> EventResponse {
        let selected = self.selection.get();
        if let Some(id) = selected {
            let layout_node = layout.get(id).unwrap();

            if layout_node
                .event_interest
                .contains(EventInterest::FOCUSED_KEYBOARD)
            {
                let mut node = dom.get_mut(id).unwrap();
                let event = WidgetEvent::TextInput(c);
                return self.fire_event(dom, layout, id, &mut node, &event);
            }
        }

        EventResponse::Bubble
    }

    fn send_button_change(
        &self,
        dom: &Dom,
        layout: &LayoutDom,
        button: MouseButton,
        down: bool,
    ) -> EventResponse {
        let mouse = self.mouse.borrow();
        let intersections = self.intersections.borrow();
        let mut overall_response = EventResponse::Bubble;

        for &id in &intersections.mouse_hit {
            if let Some(mut node) = dom.get_mut(id) {
                let event = WidgetEvent::MouseButtonChanged {
                    button,
                    down,
                    inside: true,
                    position: mouse.position.unwrap_or(Vec2::ZERO) / layout.scale_factor(),
                    modifiers: self.modifiers.get(),
                };
                let response = self.fire_event(dom, layout, id, &mut node, &event);

                if response == EventResponse::Sink {
                    overall_response = response;
                    break;
                }
            }
        }

        // For consistency, reverse the interest_mouse array like we do in
        // hit_test. This event can't be sunk, so it's not super important.
        let interest_mouse = layout.interest_mouse.iter().copied().rev();

        for (id, interest) in interest_mouse {
            if interest.contains(EventInterest::MOUSE_OUTSIDE)
                && !intersections.mouse_hit.contains(&id)
            {
                if let Some(mut node) = dom.get_mut(id) {
                    let event = WidgetEvent::MouseButtonChanged {
                        button,
                        down,
                        inside: false,
                        position: mouse.position.unwrap_or(Vec2::ZERO) / layout.scale_factor(),
                        modifiers: self.modifiers.get(),
                    };
                    self.fire_event(dom, layout, id, &mut node, &event);
                }
            }
        }

        overall_response
    }

    fn send_mouse_scroll(&self, dom: &Dom, layout: &LayoutDom, delta: Vec2) -> EventResponse {
        let intersections = self.intersections.borrow();

        let mut overall_response = EventResponse::Bubble;

        for &id in &intersections.mouse_hit {
            if let Some(mut node) = dom.get_mut(id) {
                let event = WidgetEvent::MouseScroll { delta };
                let response = self.fire_event(dom, layout, id, &mut node, &event);

                if response == EventResponse::Sink {
                    overall_response = response;
                    break;
                }
            }
        }

        overall_response
    }

    fn send_mouse_move(&self, dom: &Dom, layout: &LayoutDom) {
        let mouse = self.mouse.borrow();
        let interest_mouse = layout.interest_mouse.iter().copied().rev();

        let pos = mouse.position.map(|pos| pos / layout.scale_factor());
        let event = WidgetEvent::MouseMoved(pos);

        for (id, interest) in interest_mouse {
            if interest.intersects(EventInterest::MOUSE_MOVE) {
                let mut node = dom.get_mut(id).unwrap();
                self.fire_event(dom, layout, id, &mut node, &event);
            }
        }
    }

    fn send_mouse_enter(&self, dom: &Dom, layout: &LayoutDom) {
        let mut intersections = self.intersections.borrow_mut();
        let intersections = &mut *intersections;

        for &hit in &intersections.mouse_hit {
            if let Some(mut node) = dom.get_mut(hit) {
                if !intersections.mouse_entered.contains(&hit) {
                    intersections.mouse_entered.push(hit);

                    let response =
                        self.fire_event(dom, layout, hit, &mut node, &WidgetEvent::MouseEnter);

                    if response == EventResponse::Sink {
                        intersections.mouse_entered_and_sunk.push(hit);
                        break;
                    }
                } else if intersections.mouse_entered_and_sunk.contains(&hit) {
                    // This widget was hovered previously, is still hovered, and
                    // sunk the mouse enter event before. In order to prevent
                    // erroneously hovering other widgets, continue sinking this
                    // event.
                    break;
                }
            }
        }
    }

    fn send_mouse_leave(&self, dom: &Dom, layout: &LayoutDom) {
        let mut intersections = self.intersections.borrow_mut();

        let mut to_remove = SmallVec::<[WidgetId; 4]>::new();

        for &hit in &intersections.mouse_entered {
            if !intersections.mouse_hit.contains(&hit) {
                if let Some(mut node) = dom.get_mut(hit) {
                    self.fire_event(dom, layout, hit, &mut node, &WidgetEvent::MouseLeave);
                }

                to_remove.push(hit);
            }
        }

        for remove in to_remove {
            intersections.mouse_entered.retain(|&id| id != remove);
            intersections
                .mouse_entered_and_sunk
                .retain(|&id| id != remove);
        }
    }

    fn mouse_hit_test(&self, dom: &Dom, layout: &LayoutDom) {
        let mut intersections = self.intersections.borrow_mut();
        let mouse = self.mouse.borrow();

        intersections.mouse_hit.clear();

        if let Some(mut mouse_pos) = mouse.position {
            mouse_pos /= layout.scale_factor();
            hit_test(dom, layout, mouse_pos, &mut intersections.mouse_hit);
        }
    }

    fn settle_buttons(&self) {
        let mut mouse = self.mouse.borrow_mut();

        for state in mouse.buttons.values_mut() {
            state.settle();
        }
    }

    /// Notify the widget of an event, pushing it onto the stack first to ensure
    /// that the DOM will have the correct widget at the top of the stack if
    /// queried.
    fn fire_event(
        &self,
        dom: &Dom,
        layout: &LayoutDom,
        id: WidgetId,
        node: &mut DomNode,
        event: &WidgetEvent,
    ) -> EventResponse {
        let context = EventContext {
            dom,
            layout,
            input: self,
        };

        dom.enter(id);
        let response = node.widget.event(context, event);
        dom.exit(id);

        response
    }
}

#[profiling::function]
fn hit_test(_dom: &Dom, layout: &LayoutDom, coords: Vec2, output: &mut Vec<WidgetId>) {
    // interest_mouse is stored in layout traversal order, which is depth first.
    //
    // We want to test against the deepest widgets in the tree first and bubble
    // to their ancestors first.
    let interest_mouse = layout.interest_mouse.iter().copied().rev();

    for (id, _interest) in interest_mouse {
        let layout_node = layout.get(id).unwrap();

        let mut rect = layout_node.rect;
        let mut node = layout_node;
        while let Some(parent) = node.clipped_by {
            node = layout.get(parent).unwrap();
            rect = rect.constrain(node.rect);
        }

        if rect.contains_point(coords) {
            output.push(id);
        }
    }
}
