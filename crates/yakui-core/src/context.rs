//! Provides access to the currently active DOM or input context.

use std::cell::{Ref, RefCell, RefMut};
use std::thread::LocalKey;

use crate::dom::Dom;
use crate::input::InputState;

type Storage<T> = RefCell<Option<T>>;

thread_local! {
    static CURRENT_DOM: Storage<Dom> = RefCell::new(None);
    static CURRENT_INPUT: Storage<InputState> = RefCell::new(None);
}

/// Select the currently active widget.
pub fn capture_selection() {
    let id = dom().current();
    input().set_selection(Some(id));
}

/// Deselect the currently active widget.
pub fn remove_selection() {
    input().set_selection(None);
}

/// Tells whether the current widget is selected.
pub fn is_selected() -> bool {
    let id = dom().current();
    input().selection() == Some(id)
}

/// Potentially initialize and then get the value of some topologically-aware
/// state.
pub fn use_state<T, F>(init: F) -> RefMut<'static, T>
where
    T: 'static,
    F: FnOnce() -> T,
{
    dom().get_state_or_init(init)
}

/// If there is a DOM currently being updated on this thread, returns a
/// reference to it.
///
/// # Panics
/// Panics if there is no DOM currently being updated on this thread.
pub fn dom() -> Ref<'static, Dom> {
    borrow(&CURRENT_DOM)
}

pub(crate) fn bind_dom(dom: &Dom) {
    bind(&CURRENT_DOM, dom.clone());
}

pub(crate) fn unbind_dom() {
    unbind(&CURRENT_DOM);
}

/// If there is an input context available on this thread, returns a reference
/// to it.
///
/// # Panics
/// Panics if there is no DOM currently being updated on this thread.
pub fn input() -> Ref<'static, InputState> {
    borrow(&CURRENT_INPUT)
}

pub(crate) fn bind_input(input: &InputState) {
    bind(&CURRENT_INPUT, input.clone());
}

pub(crate) fn unbind_input() {
    unbind(&CURRENT_INPUT);
}

/// Borrow a thread local storage and allow it to be held for 'static.
fn borrow<T>(storage: &'static LocalKey<Storage<T>>) -> Ref<'static, T> {
    storage.with(|context| {
        // SAFETY: Rust won't give us a 'static reference here because this
        // thread could send the 'static value to another thread and then die
        // prematurely. This would be bad.
        //
        // However, we have some good news! Ref<T> is not Send and never will
        // be. We can rest assured that this reference will never outlive the
        // thread it originated from.
        let context = unsafe { extend_lifetime(context) };

        let context = context.borrow();
        Ref::map(context, |context| {
            context.as_ref().unwrap_or_else(|| {
                panic!("Cannot get access to a Dom when one is not in progress.")
            })
        })
    })
}

fn bind<T>(storage: &'static LocalKey<Storage<T>>, value: T) {
    storage.with(move |current| {
        let mut current = current.borrow_mut();
        if current.is_some() {
            panic!("Cannot start a Dom while one is already in progress.");
        }
        *current = Some(value);
    });
}

fn unbind<T>(storage: &'static LocalKey<Storage<T>>) {
    storage.with(|current| {
        let mut current = current.borrow_mut();
        current.take().unwrap_or_else(|| {
            panic!("Cannot stop a Dom when there is not one in progress.");
        });
    });
}

unsafe fn extend_lifetime<'a, T>(value: &'a T) -> &'static T {
    std::mem::transmute::<&'a T, &'static T>(value)
}
