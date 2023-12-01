//! Defines yakui's DOM, which holds the hierarchy of widgets and their
//! implementation details.

mod debug;
mod dummy;
mod root;

use std::any::{type_name, TypeId};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::VecDeque;
use std::mem::replace;
use std::rc::Rc;

use anymap::AnyMap;
use thunderdome::Arena;

use crate::id::WidgetId;
use crate::response::Response;
use crate::widget::{ErasedWidget, Widget};

use self::dummy::DummyWidget;
use self::root::RootWidget;

/// The DOM that contains the tree of active widgets.
pub struct Dom {
    inner: Rc<DomInner>,
}

struct DomInner {
    nodes: RefCell<Arena<DomNode>>,
    stack: RefCell<Vec<WidgetId>>,
    removed_nodes: RefCell<Vec<WidgetId>>,
    root: WidgetId,
    globals: RefCell<AnyMap>,
}

/// A node in the [`Dom`].
pub struct DomNode {
    /// The widget implementation. Only a subset of the methods from [`Widget`]
    /// are available without downcasting the widget first.
    pub widget: Box<dyn ErasedWidget>,

    /// The parent of this node, if it has one.
    pub parent: Option<WidgetId>,

    /// All of this node's children.
    pub children: Vec<WidgetId>,

    /// Used when building the tree. The index of the next child if a new child
    /// starts being built.
    next_child: usize,
}

impl Dom {
    /// Create a new, empty DOM.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(DomInner::new()),
        }
    }

    pub(crate) fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }

    /// Start the build phase for the DOM and bind it to the current thread.
    pub fn start(&self) {
        log::debug!("Dom::start()");

        let mut nodes = self.inner.nodes.borrow_mut();
        let root = nodes.get_mut(self.inner.root.index()).unwrap();
        root.next_child = 0;
    }

    /// End the DOM's build phase.
    pub fn finish(&self) {
        log::debug!("Dom::finish()");

        let mut nodes = self.inner.nodes.borrow_mut();
        let mut removed_nodes = self.inner.removed_nodes.borrow_mut();
        removed_nodes.clear();

        let root = self.inner.root;
        trim_children(&mut nodes, &mut removed_nodes, root);
    }

    /// Tells how many nodes are currently in the DOM.
    pub fn len(&self) -> usize {
        self.inner.nodes.borrow().len()
    }

    /// Tells whether the DOM is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.nodes.borrow().is_empty()
    }

    /// Gives the root widget in the DOM. This widget will always exist.
    pub fn root(&self) -> WidgetId {
        self.inner.root
    }

    /// Gives a list of all of the nodes that were removed in the last update.
    /// This is used for synchronizing state with the primary DOM storage.
    pub(crate) fn removed_nodes(&self) -> Ref<'_, [WidgetId]> {
        let vec = self.inner.removed_nodes.borrow();
        Ref::map(vec, AsRef::as_ref)
    }

    /// Enter the context of the given widget, pushing it onto the stack so that
    /// [`Dom::current`] will report the correct widget.
    pub(crate) fn enter(&self, id: WidgetId) {
        self.inner.stack.borrow_mut().push(id);
    }

    /// Pop the given widget off of the traversal stack. Panics if the widget on
    /// top of the stack is not the one with the given ID.
    pub(crate) fn exit(&self, id: WidgetId) {
        assert_eq!(self.inner.stack.borrow_mut().pop(), Some(id));
    }

    /// If the DOM is being built, tells which widget is currently being built.
    ///
    /// This method only gives valid results when called from inside a
    /// [`Widget`] lifecycle method.
    pub fn current(&self) -> WidgetId {
        let stack = self.inner.stack.borrow();
        stack.last().copied().unwrap_or(self.inner.root)
    }

    /// Returns a reference to the current DOM node. See [`Dom::current`].
    pub fn get_current(&self) -> Ref<'_, DomNode> {
        let nodes = self.inner.nodes.borrow();
        let index = self.current().index();

        Ref::map(nodes, |nodes| nodes.get(index).unwrap())
    }

    /// Tells whether the DOM contains a widget with the given ID.
    pub fn contains(&self, id: WidgetId) -> bool {
        let nodes = self.inner.nodes.borrow();
        let index = id.index();
        nodes.contains(index)
    }

    /// Get the node with the given widget ID.
    pub fn get(&self, id: WidgetId) -> Option<Ref<'_, DomNode>> {
        let nodes = self.inner.nodes.borrow();
        let index = id.index();

        if nodes.contains(index) {
            Some(Ref::map(nodes, |nodes| nodes.get(index).unwrap()))
        } else {
            None
        }
    }

    /// Get a mutable reference to the node with the given widget ID.
    pub fn get_mut(&self, id: WidgetId) -> Option<RefMut<'_, DomNode>> {
        let nodes = self.inner.nodes.borrow_mut();
        let index = id.index();

        if nodes.contains(index) {
            Some(RefMut::map(nodes, |nodes| nodes.get_mut(index).unwrap()))
        } else {
            None
        }
    }

    /// Get a piece of DOM-global state or initialize it with the given
    /// function.
    ///
    /// This is intended for any state that is global. It's not a perfect fit
    /// for scoped state like themes.
    pub fn get_global_or_init<T, F>(&self, init: F) -> T
    where
        T: 'static + Clone,
        F: FnOnce() -> T,
    {
        let mut globals = self.inner.globals.borrow_mut();
        globals.entry::<T>().or_insert_with(init).clone()
    }

    /// Convenience method for calling [`Dom::begin_widget`] immediately
    /// followed by [`Dom::end_widget`].
    pub fn do_widget<T: Widget>(&self, props: T::Props<'_>) -> Response<T::Response> {
        let response = self.begin_widget::<T>(props);
        self.end_widget::<T>(response.id);
        response
    }

    /// Begin building a widget with the given type and props.
    ///
    /// After calling this method, children can be added to this widget.
    pub fn begin_widget<T: Widget>(&self, props: T::Props<'_>) -> Response<T::Response> {
        log::trace!("begin_widget::<{}>({props:#?}", type_name::<T>());

        let parent_id = self.current();

        let (id, mut widget) = {
            let mut nodes = self.inner.nodes.borrow_mut();

            if let Some(id) = next_existing_widget(&mut nodes, parent_id) {
                // There is an existing child in this slot. It may or may not
                // match up with the widget we're starting here.

                // Component::update needs mutable access to both the widget and the
                // DOM, so we need to rip the widget out of the tree so we can
                // release our lock.
                let node = nodes.get_mut(id.index()).unwrap();
                let widget = replace(&mut node.widget, Box::new(DummyWidget));

                if widget.as_ref().type_id() == TypeId::of::<T>() {
                    // happy path! we can update our widget in place.

                    node.next_child = 0;
                    (id, widget)
                } else {
                    // sad path! the widget changed type, so we have to burn
                    // down the world and try again.

                    let mut removed_nodes = self.inner.removed_nodes.borrow_mut();
                    remove_recursive(&mut nodes, &mut removed_nodes, id);

                    new_widget::<T>(&mut nodes, parent_id)
                }
            } else {
                // we're in uncharted territory!

                new_widget::<T>(&mut nodes, parent_id)
            }
        };

        // After this point, we've officially entered our widget.
        self.inner.stack.borrow_mut().push(id);

        // Update the widget now that we've released our locks.
        let response = {
            let widget = widget.downcast_mut::<T>().unwrap();
            widget.update(props)
        };

        // Quick! Put the widget back, before anyone notices!
        {
            let mut nodes = self.inner.nodes.borrow_mut();
            let node = nodes.get_mut(id.index()).unwrap();
            node.widget = widget;
        }

        Response::new(id, response)
    }

    /// Finish building the widget with the given ID. Must be the top of the
    /// stack, with no other widgets pending.
    pub fn end_widget<T: Widget>(&self, id: WidgetId) {
        log::trace!("end_widget::<{}>({id:?})", type_name::<T>());

        let old_top = self.inner.stack.borrow_mut().pop().unwrap_or_else(|| {
            panic!("Cannot end_widget without an in-progress widget.");
        });

        assert!(
            id == old_top,
            "Dom::end_widget did not match the input widget."
        );

        let mut nodes = self.inner.nodes.borrow_mut();
        let mut removed_nodes = self.inner.removed_nodes.borrow_mut();
        trim_children(&mut nodes, &mut removed_nodes, id);
    }
}

impl DomInner {
    fn new() -> Self {
        let mut nodes = Arena::new();
        let root = nodes.insert(DomNode {
            widget: Box::new(RootWidget),
            parent: None,
            children: Vec::new(),
            next_child: 0,
        });

        Self {
            globals: RefCell::new(AnyMap::new()),
            nodes: RefCell::new(nodes),
            removed_nodes: RefCell::new(Vec::new()),
            stack: RefCell::new(Vec::new()),
            root: WidgetId::new(root),
        }
    }
}

fn next_existing_widget(nodes: &mut Arena<DomNode>, parent_id: WidgetId) -> Option<WidgetId> {
    let parent = nodes.get_mut(parent_id.index()).unwrap();

    if let Some(&id) = parent.children.get(parent.next_child) {
        parent.next_child += 1;
        Some(id)
    } else {
        None
    }
}

fn new_widget<T: Widget>(
    nodes: &mut Arena<DomNode>,
    parent_id: WidgetId,
) -> (WidgetId, Box<dyn ErasedWidget>) {
    let index = nodes.insert(DomNode {
        widget: Box::new(DummyWidget),
        parent: Some(parent_id),
        children: Vec::new(),
        next_child: 0,
    });

    let id = WidgetId::new(index);

    let parent = nodes.get_mut(parent_id.index()).unwrap();

    if parent.next_child < parent.children.len() {
        parent.children[parent.next_child] = id;
    } else {
        parent.children.push(id);
    }

    parent.next_child += 1;

    let widget = Box::new(T::new());

    (id, widget)
}

/// Remove children from the given node that weren't present in the latest
/// traversal through the tree.
fn trim_children(nodes: &mut Arena<DomNode>, removed_nodes: &mut Vec<WidgetId>, id: WidgetId) {
    let node = nodes.get_mut(id.index()).unwrap();

    if node.next_child < node.children.len() {
        let mut queue: VecDeque<WidgetId> = VecDeque::new();
        let to_drop = &node.children[node.next_child..];
        queue.extend(to_drop);
        removed_nodes.extend_from_slice(to_drop);

        node.children.truncate(node.next_child);

        while let Some(child_id) = queue.pop_front() {
            removed_nodes.push(child_id);
            let child = nodes.remove(child_id.index()).unwrap();
            queue.extend(child.children);
        }
    }
}

/// Remove a widget and all of its descendants recursively.
fn remove_recursive(nodes: &mut Arena<DomNode>, removed_nodes: &mut Vec<WidgetId>, id: WidgetId) {
    let mut queue = VecDeque::new();
    queue.push_back(id);

    while let Some(id) = queue.pop_front() {
        removed_nodes.push(id);

        let Some(node) = nodes.get(id.index()) else {
            continue;
        };

        let to_drop = node.children.as_slice();
        queue.extend(to_drop);

        nodes.remove(id.index());
    }
}
