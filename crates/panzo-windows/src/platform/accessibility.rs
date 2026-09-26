//! UI Automation for the owner-drawn host. COM only reads snapshots and queues commands;
//! it never borrows the editor, calls media code, or waits for the UI thread.
#![allow(non_snake_case, non_upper_case_globals)]
// COM names and Windows constants.
// Emitted by windows-core's interface/implement macros, not handwritten pointer conversions.
#![allow(
    clippy::transmute_ptr_to_ptr,
    clippy::ref_as_ptr,
    clippy::inline_always
)]

use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use windows::Win32::Foundation::{
    E_INVALIDARG, E_OUTOFMEMORY, E_POINTER, HWND, LPARAM, LRESULT, POINT, RECT, S_OK, WPARAM,
};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::SAFEARRAY;
use windows::Win32::System::Ole::{SafeArrayCreateVector, SafeArrayDestroy, SafeArrayPutElement};
use windows::Win32::System::Variant::{VARIANT, VT_I4, VT_UNKNOWN};
use windows::Win32::UI::Accessibility::UIA_Invoke_InvokedEventId;
use windows::Win32::UI::Accessibility::{
    IInvokeProvider, IInvokeProvider_Impl, IRangeValueProvider, IRangeValueProvider_Impl,
    IRawElementProviderFragment, IRawElementProviderFragmentRoot, IRawElementProviderSimple,
    IToggleProvider, IToggleProvider_Impl, NavigateDirection, NavigateDirection_FirstChild,
    NavigateDirection_LastChild, NavigateDirection_NextSibling, NavigateDirection_Parent,
    NavigateDirection_PreviousSibling, ProviderOptions, ProviderOptions_ProviderOwnsSetFocus,
    ProviderOptions_ServerSideProvider, StructureChangeType_ChildrenInvalidated, ToggleState,
    ToggleState_Off, ToggleState_On, UIA_AutomationFocusChangedEventId, UIA_AutomationIdPropertyId,
    UIA_ButtonControlTypeId, UIA_CheckBoxControlTypeId, UIA_ControlTypePropertyId,
    UIA_E_ELEMENTNOTAVAILABLE, UIA_E_ELEMENTNOTENABLED, UIA_E_INVALIDOPERATION, UIA_E_NOTSUPPORTED,
    UIA_FrameworkIdPropertyId, UIA_HasKeyboardFocusPropertyId, UIA_HelpTextPropertyId,
    UIA_InvokePatternId, UIA_IsContentElementPropertyId, UIA_IsControlElementPropertyId,
    UIA_IsEnabledPropertyId, UIA_IsInvokePatternAvailablePropertyId,
    UIA_IsKeyboardFocusablePropertyId, UIA_IsOffscreenPropertyId,
    UIA_IsRangeValuePatternAvailablePropertyId, UIA_IsTogglePatternAvailablePropertyId,
    UIA_NamePropertyId, UIA_PATTERN_ID, UIA_PROPERTY_ID, UIA_PaneControlTypeId,
    UIA_RangeValuePatternId, UIA_RangeValueValuePropertyId, UIA_SliderControlTypeId,
    UIA_TextControlTypeId, UIA_TogglePatternId, UIA_ToggleToggleStatePropertyId,
    UiaAppendRuntimeId, UiaClientsAreListening, UiaHostProviderFromHwnd, UiaRaiseAutomationEvent,
    UiaRaiseStructureChangedEvent, UiaRect, UiaReturnRawElementProvider,
};
use windows::Win32::UI::Accessibility::{
    ISelectionItemProvider, ISelectionItemProvider_Impl, ISelectionProvider,
    ISelectionProvider_Impl, UIA_IsSelectionItemPatternAvailablePropertyId,
    UIA_IsSelectionPatternAvailablePropertyId, UIA_ListItemControlTypeId,
    UIA_SelectionItem_ElementSelectedEventId, UIA_SelectionItemIsSelectedPropertyId,
    UIA_SelectionItemPatternId, UIA_SelectionPatternId, UiaRaiseAutomationPropertyChangedEvent,
};
use windows::Win32::UI::Accessibility::{
    UIA_RangeValueIsReadOnlyPropertyId, UIA_RangeValueLargeChangePropertyId,
    UIA_RangeValueMaximumPropertyId, UIA_RangeValueMinimumPropertyId,
    UIA_RangeValueSmallChangePropertyId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, IsWindowEnabled};
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, PostMessageW, WM_APP};
use windows::core::{
    BOOL, Error, HRESULT, IUnknown, IUnknown_Vtbl, Interface, Result, implement, interface,
};

pub const WM_ACCESS_ACTION: u32 = WM_APP + 0x4a0;
const WM_ACCESS_NOTIFY: u32 = WM_APP + 0x4a1;
pub const PLAYHEAD_ID: u32 = 100;
pub const STATUS_ID: u32 = 101;
const MAX_PENDING: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Invoke(u32),
    Focus(u32),
    SetRange(u32, f64),
    Select(u32),
    ClearSelection(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Button,
    Toggle(bool),
    Range {
        value: f64,
        minimum: f64,
        maximum: f64,
    },
    Text,
    Item(bool),
}

#[derive(Clone)]
enum PropertyValue {
    Text(String),
    Bool(bool),
    Number(f64),
    Int(i32),
}
impl PropertyValue {
    fn variant(&self) -> VARIANT {
        match self {
            Self::Text(s) => s.as_str().into(),
            Self::Bool(b) => (*b).into(),
            Self::Number(n) => (*n).into(),
            Self::Int(n) => (*n).into(),
        }
    }
}
type PropertyChange = (u32, UIA_PROPERTY_ID, PropertyValue, PropertyValue);

#[derive(Default)]
struct Identities {
    by_key: HashMap<String, u32>,
    keys: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: u32,
    pub name: String,
    pub help: String,
    pub rect: RECT,
    pub enabled: bool,
    pub kind: Kind,
}

impl Node {
    pub fn button(id: u32, name: &str, help: &str, rect: RECT, enabled: bool) -> Self {
        Self {
            id,
            name: name.into(),
            help: help.into(),
            rect,
            enabled,
            kind: Kind::Button,
        }
    }

    fn focusable(&self) -> bool {
        self.enabled && self.kind != Kind::Text
    }
}

#[derive(Default)]
#[allow(clippy::struct_excessive_bools)] // Independent window, queue and event flags.
struct Snapshot {
    hwnd: usize,
    alive: bool,
    enabled: bool,
    focused: Option<u32>,
    origin: POINT,
    root_rect: RECT,
    name: String,
    nodes: Vec<Node>,
    pending: VecDeque<Action>,
    action_posted: bool,
    notify_pending: bool,
    focus_changed: bool,
    structure_changed: bool,
    properties: Vec<PropertyChange>,
    invoked: Vec<u32>,
}

struct Shared {
    generation: u32,
    state: Mutex<Snapshot>,
    identities: Mutex<Identities>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Snapshot> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn registry() -> &'static Mutex<HashMap<usize, Weak<Shared>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<usize, Weak<Shared>>>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

pub struct AccessibilityHost(Arc<Shared>);

impl Default for AccessibilityHost {
    fn default() -> Self {
        static GENERATION: AtomicU32 = AtomicU32::new(1);
        Self(Arc::new(Shared {
            generation: GENERATION.fetch_add(1, Ordering::Relaxed),
            state: Mutex::default(),
            identities: Mutex::default(),
        }))
    }
}

impl AccessibilityHost {
    /// Retain the current page's COM provider across navigation in regression tests.
    #[cfg(test)]
    pub(crate) fn review_invoke_provider(&self, id: u32) -> Result<IInvokeProvider> {
        provider(&self.0, id).cast()
    }

    /// Invalidate a page's providers before reusing the same HWND for another page.
    /// A fresh Shared/generation prevents retained providers becoming valid again
    /// when this host later publishes controls with the same local IDs.
    pub fn detach(&mut self) {
        *self = Self::default();
    }

    pub fn blur(&self) {
        let mut s = self.0.lock();
        if let Some(id) = s.focused.take() {
            s.focus_changed = true;
            if s.properties.len() < 128 {
                s.properties.push((
                    id,
                    UIA_HasKeyboardFocusPropertyId,
                    PropertyValue::Bool(true),
                    PropertyValue::Bool(false),
                ));
            }
            if !s.notify_pending {
                s.notify_pending = unsafe {
                    PostMessageW(
                        Some(HWND(s.hwnd as *mut _)),
                        WM_ACCESS_NOTIFY,
                        WPARAM(0),
                        LPARAM(0),
                    )
                }
                .is_ok();
            }
        }
    }
    pub fn invoked(&self, id: u32) {
        let mut s = self.0.lock();
        if s.alive && s.invoked.len() < MAX_PENDING {
            s.invoked.push(id);
            if !s.notify_pending {
                s.notify_pending = unsafe {
                    PostMessageW(
                        Some(HWND(s.hwnd as *mut _)),
                        WM_ACCESS_NOTIFY,
                        WPARAM(0),
                        LPARAM(0),
                    )
                }
                .is_ok();
            }
        }
    }
    pub fn stable_id(&self, key: String) -> u32 {
        let mut ids = self
            .0
            .identities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(id) = ids.by_key.get(&key) {
            return *id;
        }
        let id = u32::try_from(ids.keys.len()).expect("UIA identity count exceeds u32") + 1000;
        ids.keys.push(key.clone());
        ids.by_key.insert(key, id);
        id
    }

    pub fn key(&self, id: u32) -> Option<String> {
        let index = usize::try_from(id.checked_sub(1000)?).ok()?;
        self.0
            .identities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys
            .get(index)
            .cloned()
    }

    pub fn focus_order(&self) -> Vec<u32> {
        self.0
            .lock()
            .nodes
            .iter()
            .filter(|node| node.focusable())
            .map(|node| node.id)
            .collect()
    }

    pub fn node_rect(&self, id: u32) -> Option<RECT> {
        self.0
            .lock()
            .nodes
            .iter()
            .find(|node| node.id == id)
            .map(|node| node.rect)
    }
    /// Called exclusively by the owner thread. Empty/clipped controls are omitted, not exposed
    /// as invisible operable buttons. Screen conversion uses physical, per-monitor-aware pixels.
    pub fn publish(
        &self,
        hwnd: HWND,
        name: &str,
        client: RECT,
        mut nodes: Vec<Node>,
        focus: Option<u32>,
    ) {
        let mut origin = POINT::default();
        let _ = unsafe { ClientToScreen(hwnd, &raw mut origin) };
        for node in &mut nodes {
            node.rect = clipped(node.rect, client);
        }
        // Timeline items remain discoverable/selected while outside the viewport. Hidden
        // command buttons are omitted; selecting an offscreen item scrolls it into view.
        nodes.retain(|node| {
            matches!(node.kind, Kind::Item(_))
                || node.rect.right > node.rect.left && node.rect.bottom > node.rect.top
        });
        nodes.sort_by_key(|node| (node.rect.top, node.rect.left));
        let enabled = unsafe { IsWindowEnabled(hwnd).as_bool() && !IsIconic(hwnd).as_bool() };
        let focused = if unsafe { GetFocus() } == hwnd {
            focus
                .filter(|id| nodes.iter().any(|node| node.id == *id && node.focusable()))
                .or(Some(0))
        } else {
            None
        };
        let mut s = self.0.lock();
        let first = s.hwnd == 0;
        let was_enabled = s.enabled;
        s.focus_changed |= s.focused != focused;
        s.structure_changed |= s
            .nodes
            .iter()
            .map(|node| node.id)
            .ne(nodes.iter().map(|node| node.id));
        s.hwnd = hwnd.0 as usize;
        s.alive = true;
        s.enabled = enabled;
        s.focused = focused;
        s.origin = origin;
        s.root_rect = client;
        s.name = name.into();
        let changes = property_changes(&s.nodes, &nodes, was_enabled, enabled);
        for change in changes {
            if let Some(previous) = s
                .properties
                .iter_mut()
                .find(|old| old.0 == change.0 && old.1 == change.1)
            {
                previous.3 = change.3;
            } else if s.properties.len() < 128 {
                s.properties.push(change);
            }
        }
        s.nodes = nodes;
        if !s.notify_pending && (s.focus_changed || s.structure_changed || !s.properties.is_empty())
        {
            s.notify_pending =
                unsafe { PostMessageW(Some(hwnd), WM_ACCESS_NOTIFY, WPARAM(0), LPARAM(0)) }.is_ok();
        }
        drop(s);
        if first {
            registry()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(hwnd.0 as usize, Arc::downgrade(&self.0));
        }
    }

    pub fn take_actions(&self) -> Vec<Action> {
        let mut state = self.0.lock();
        state.action_posted = false;
        state.pending.drain(..).collect()
    }
}

impl Drop for AccessibilityHost {
    fn drop(&mut self) {
        let mut s = self.0.lock();
        let hwnd = s.hwnd;
        *s = Snapshot::default();
        drop(s);
        let mut registry = registry()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if registry
            .get(&hwnd)
            .and_then(Weak::upgrade)
            .is_some_and(|entry| Arc::ptr_eq(&entry, &self.0))
        {
            registry.remove(&hwnd);
        }
    }
}

/// Intercept before borrowing `WindowState`: UIA can re-enter `WM_GETOBJECT` while a modal or
/// an accessibility event is running. No Rust mutable UI reference is touched here.
pub fn window_message(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    use windows::Win32::UI::WindowsAndMessaging::WM_GETOBJECT;
    let blocked_action = message == WM_ACCESS_ACTION && !unsafe { IsWindowEnabled(hwnd).as_bool() };
    if message != WM_GETOBJECT && message != WM_ACCESS_NOTIFY && !blocked_action {
        return None;
    }
    let shared = registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&(hwnd.0 as usize))
        .and_then(Weak::upgrade)?;
    if blocked_action {
        // A command queued just before a native modal opened must not re-enter the borrowed
        // owner state via its nested message pump, or replay unexpectedly after it closes.
        let mut s = shared.lock();
        s.pending.clear();
        s.action_posted = false;
        return Some(LRESULT(0));
    }
    if message == WM_GETOBJECT {
        let root = provider(&shared, 0)
            .cast::<IRawElementProviderSimple>()
            .ok()?;
        return Some(unsafe { UiaReturnRawElementProvider(hwnd, wparam, lparam, &root) });
    }
    let (focus, structure, properties, invoked) = {
        let mut s = shared.lock();
        s.notify_pending = false;
        let focus = std::mem::take(&mut s.focus_changed)
            .then_some(s.focused)
            .flatten();
        (
            focus,
            std::mem::take(&mut s.structure_changed),
            std::mem::take(&mut s.properties),
            std::mem::take(&mut s.invoked),
        )
    };
    if unsafe { UiaClientsAreListening().as_bool() } {
        for id in invoked {
            if let Ok(node) = provider(&shared, id).cast::<IRawElementProviderSimple>() {
                let _ = unsafe { UiaRaiseAutomationEvent(&node, UIA_Invoke_InvokedEventId) };
            }
        }
        for (id, property, old, new) in properties {
            if let Ok(node) = provider(&shared, id).cast::<IRawElementProviderSimple>() {
                let _ = unsafe {
                    UiaRaiseAutomationPropertyChangedEvent(
                        &node,
                        property,
                        &old.variant(),
                        &new.variant(),
                    )
                };
                if property == UIA_SelectionItemIsSelectedPropertyId
                    && matches!(new, PropertyValue::Bool(true))
                {
                    let _ = unsafe {
                        UiaRaiseAutomationEvent(&node, UIA_SelectionItem_ElementSelectedEventId)
                    };
                }
            }
        }
        if let Some(id) = focus
            && let Ok(node) = provider(&shared, id).cast::<IRawElementProviderSimple>()
        {
            let _ = unsafe { UiaRaiseAutomationEvent(&node, UIA_AutomationFocusChangedEventId) };
        }
        if structure && let Ok(root) = provider(&shared, 0).cast::<IRawElementProviderSimple>() {
            let _ = unsafe {
                UiaRaiseStructureChangedEvent(
                    &root,
                    StructureChangeType_ChildrenInvalidated,
                    std::ptr::null_mut(),
                    0,
                )
            };
        }
    }
    Some(LRESULT(0))
}

fn clipped(rect: RECT, client: RECT) -> RECT {
    RECT {
        left: rect.left.max(client.left),
        top: rect.top.max(client.top),
        right: rect.right.min(client.right),
        bottom: rect.bottom.min(client.bottom),
    }
}

fn property_changes(
    before: &[Node],
    after: &[Node],
    was_enabled: bool,
    enabled: bool,
) -> Vec<PropertyChange> {
    let mut changes = Vec::new();
    let before: HashMap<_, _> = before.iter().map(|node| (node.id, node)).collect();
    for node in after {
        let Some(old) = before.get(&node.id) else {
            continue;
        };
        if old.name != node.name {
            changes.push((
                node.id,
                UIA_NamePropertyId,
                PropertyValue::Text(old.name.clone()),
                PropertyValue::Text(node.name.clone()),
            ));
        }
        if (was_enabled && old.enabled) != (enabled && node.enabled) {
            changes.push((
                node.id,
                UIA_IsEnabledPropertyId,
                PropertyValue::Bool(was_enabled && old.enabled),
                PropertyValue::Bool(enabled && node.enabled),
            ));
        }
        match (&old.kind, &node.kind) {
            (Kind::Toggle(a), Kind::Toggle(b)) if a != b => changes.push((
                node.id,
                UIA_ToggleToggleStatePropertyId,
                PropertyValue::Int(i32::from(*a)),
                PropertyValue::Int(i32::from(*b)),
            )),
            (Kind::Range { value: a, .. }, Kind::Range { value: b, .. }) if a != b => {
                changes.push((
                    node.id,
                    UIA_RangeValueValuePropertyId,
                    PropertyValue::Number(*a),
                    PropertyValue::Number(*b),
                ));
            }
            (Kind::Item(a), Kind::Item(b)) if a != b => changes.push((
                node.id,
                UIA_SelectionItemIsSelectedPropertyId,
                PropertyValue::Bool(*a),
                PropertyValue::Bool(*b),
            )),
            _ => {}
        }
        if let (Kind::Range { maximum: a, .. }, Kind::Range { maximum: b, .. }) =
            (&old.kind, &node.kind)
            && a != b
        {
            changes.push((
                node.id,
                UIA_RangeValueMaximumPropertyId,
                PropertyValue::Number(*a),
                PropertyValue::Number(*b),
            ));
        }
    }
    changes
}

fn enqueue(pending: &mut VecDeque<Action>, action: Action) -> Result<()> {
    if let (Some(Action::SetRange(old_id, value)), Action::SetRange(id, next)) =
        (pending.back_mut(), &action)
        && old_id == id
    {
        *value = *next;
        return Ok(());
    }
    if pending.len() >= MAX_PENDING {
        return Err(failure(UIA_E_INVALIDOPERATION));
    }
    pending.push_back(action);
    Ok(())
}

fn failure(code: u32) -> Error {
    HRESULT(code.cast_signed()).into()
}

// windows-rs 0.62's provider traits return non-null interface wrappers even where UIA requires
// S_OK + null (end of siblings, absent pattern/host/focus). These ABI-identical declarations
// retain nullable out-pointers; never construct a null Rust IUnknown or return fake HRESULTs.
#[interface("d6dd68d1-86fd-4332-8666-9abedea2d24c")]
unsafe trait SimpleAbi: IUnknown {
    fn ProviderOptions(&self, out: *mut ProviderOptions) -> HRESULT;
    fn GetPatternProvider(&self, id: UIA_PATTERN_ID, out: *mut *mut c_void) -> HRESULT;
    fn GetPropertyValue(&self, id: UIA_PROPERTY_ID, out: *mut VARIANT) -> HRESULT;
    fn HostRawElementProvider(&self, out: *mut *mut c_void) -> HRESULT;
}
#[interface("f7063da8-8359-439c-9297-bbc5299a7d87")]
unsafe trait FragmentAbi: IUnknown {
    fn Navigate(&self, direction: NavigateDirection, out: *mut *mut c_void) -> HRESULT;
    fn GetRuntimeId(&self, out: *mut *mut SAFEARRAY) -> HRESULT;
    fn BoundingRectangle(&self, out: *mut UiaRect) -> HRESULT;
    fn GetEmbeddedFragmentRoots(&self, out: *mut *mut SAFEARRAY) -> HRESULT;
    fn SetFocus(&self) -> HRESULT;
    fn FragmentRoot(&self, out: *mut *mut c_void) -> HRESULT;
}
#[interface("620ce2a5-ab8f-40a9-86cb-de3c75599b58")]
unsafe trait RootAbi: IUnknown {
    fn ElementProviderFromPoint(&self, x: f64, y: f64, out: *mut *mut c_void) -> HRESULT;
    fn GetFocus(&self, out: *mut *mut c_void) -> HRESULT;
}

#[implement(
    SimpleAbi,
    FragmentAbi,
    RootAbi,
    IInvokeProvider,
    IToggleProvider,
    IRangeValueProvider,
    ISelectionProvider,
    ISelectionItemProvider
)]
struct Provider {
    shared: Arc<Shared>,
    id: u32,
}

fn provider(shared: &Arc<Shared>, id: u32) -> SimpleAbi {
    Provider {
        shared: Arc::clone(shared),
        id,
    }
    .into()
}

impl Provider {
    fn read<T>(&self, f: impl FnOnce(&Snapshot, Option<&Node>) -> Result<T>) -> Result<T> {
        let s = self.shared.lock();
        if !s.alive {
            return Err(failure(UIA_E_ELEMENTNOTAVAILABLE));
        }
        let node = s.nodes.iter().find(|n| n.id == self.id);
        if self.id != 0 && node.is_none() {
            return Err(failure(UIA_E_ELEMENTNOTAVAILABLE));
        }
        f(&s, node)
    }

    fn queue(&self, action: Action) -> Result<()> {
        let mut s = self.shared.lock();
        if !s.alive {
            return Err(failure(UIA_E_ELEMENTNOTAVAILABLE));
        }
        let node = s.nodes.iter().find(|node| node.id == self.id);
        if self.id != 0 && node.is_none() {
            return Err(failure(UIA_E_ELEMENTNOTAVAILABLE));
        }
        if !s.enabled
            || !unsafe { IsWindowEnabled(HWND(s.hwnd as *mut _)).as_bool() }
            || node.is_some_and(|node| !node.focusable())
        {
            return Err(failure(UIA_E_ELEMENTNOTENABLED));
        }
        // Coalesce only adjacent ranges; a Split/Invoke between seeks is an ordering barrier.
        enqueue(&mut s.pending, action)?;
        if !s.action_posted {
            if let Err(error) = unsafe {
                PostMessageW(
                    Some(HWND(s.hwnd as *mut _)),
                    WM_ACCESS_ACTION,
                    WPARAM(0),
                    LPARAM(0),
                )
            } {
                s.pending.pop_back();
                return Err(error);
            }
            s.action_posted = true;
        }
        Ok(())
    }

    fn range(&self) -> Result<(f64, f64, f64)> {
        self.read(|_, node| match node.map(|n| &n.kind) {
            Some(Kind::Range {
                value,
                minimum,
                maximum,
            }) => Ok((*value, *minimum, *maximum)),
            _ => Err(failure(UIA_E_NOTSUPPORTED)),
        })
    }

    fn property(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> {
        self.read(|s, node| {
            let kind = node.map(|n| &n.kind);
            let text = kind == Some(&Kind::Text);
            Ok(match id {
                UIA_NamePropertyId => {
                    VARIANT::from(node.map_or(s.name.as_str(), |n| n.name.as_str()))
                }
                UIA_AutomationIdPropertyId => VARIANT::from(format!("panzo-{}", self.id).as_str()),
                UIA_FrameworkIdPropertyId => VARIANT::from("PanzoWin32"),
                UIA_HelpTextPropertyId => {
                    VARIANT::from(node.map_or("录制与基础视频编辑", |n| n.help.as_str()))
                }
                UIA_ControlTypePropertyId => VARIANT::from(match kind {
                    None => UIA_PaneControlTypeId.0,
                    Some(Kind::Range { .. }) => UIA_SliderControlTypeId.0,
                    Some(Kind::Toggle(_)) => UIA_CheckBoxControlTypeId.0,
                    Some(Kind::Text) => UIA_TextControlTypeId.0,
                    Some(Kind::Item(_)) => UIA_ListItemControlTypeId.0,
                    _ => UIA_ButtonControlTypeId.0,
                }),
                UIA_IsControlElementPropertyId | UIA_IsContentElementPropertyId => {
                    VARIANT::from(true)
                }
                UIA_IsEnabledPropertyId => {
                    VARIANT::from(s.enabled && node.is_none_or(|n| n.enabled))
                }
                UIA_IsKeyboardFocusablePropertyId => {
                    VARIANT::from(s.enabled && !text && node.is_none_or(Node::focusable))
                }
                UIA_HasKeyboardFocusPropertyId => VARIANT::from(s.focused == Some(self.id)),
                UIA_IsOffscreenPropertyId => VARIANT::from(
                    node.is_some_and(|node| {
                        node.rect.right <= node.rect.left || node.rect.bottom <= node.rect.top
                    }) || !s.enabled && unsafe { IsIconic(HWND(s.hwnd as *mut _)).as_bool() },
                ),
                UIA_IsInvokePatternAvailablePropertyId => {
                    VARIANT::from(kind == Some(&Kind::Button))
                }
                UIA_IsTogglePatternAvailablePropertyId => {
                    VARIANT::from(matches!(kind, Some(Kind::Toggle(_))))
                }
                UIA_IsRangeValuePatternAvailablePropertyId => {
                    VARIANT::from(matches!(kind, Some(Kind::Range { .. })))
                }
                UIA_IsSelectionItemPatternAvailablePropertyId => {
                    VARIANT::from(matches!(kind, Some(Kind::Item(_))))
                }
                UIA_IsSelectionPatternAvailablePropertyId => VARIANT::from(
                    self.id == 0
                        && s.nodes
                            .iter()
                            .any(|node| matches!(node.kind, Kind::Item(_))),
                ),
                UIA_SelectionItemIsSelectedPropertyId => match kind {
                    Some(Kind::Item(b)) => VARIANT::from(*b),
                    _ => VARIANT::default(),
                },
                UIA_ToggleToggleStatePropertyId => match kind {
                    Some(Kind::Toggle(on)) => VARIANT::from(i32::from(*on)),
                    _ => VARIANT::default(),
                },
                UIA_RangeValueValuePropertyId => match kind {
                    Some(Kind::Range { value, .. }) => VARIANT::from(*value),
                    _ => VARIANT::default(),
                },
                UIA_RangeValueMaximumPropertyId => match kind {
                    Some(Kind::Range { maximum, .. }) => (*maximum).into(),
                    _ => VARIANT::default(),
                },
                UIA_RangeValueMinimumPropertyId => match kind {
                    Some(Kind::Range { minimum, .. }) => (*minimum).into(),
                    _ => VARIANT::default(),
                },
                UIA_RangeValueIsReadOnlyPropertyId if matches!(kind, Some(Kind::Range { .. })) => {
                    false.into()
                }
                UIA_RangeValueSmallChangePropertyId if matches!(kind, Some(Kind::Range { .. })) => {
                    (1.0 / 60.0).into()
                }
                UIA_RangeValueLargeChangePropertyId if matches!(kind, Some(Kind::Range { .. })) => {
                    5.0.into()
                }
                _ => VARIANT::default(),
            })
        })
    }
}

/// COM owns successful output interfaces/arrays. Always initialize nullable output even on error.
unsafe fn output<T: Default>(out: *mut T, result: Result<T>) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe {
        out.write(T::default());
    }
    match result {
        Ok(value) => {
            unsafe {
                out.write(value);
            }
            S_OK
        }
        Err(error) => error.code(),
    }
}

unsafe fn interface_output<T: Interface>(
    out: *mut *mut c_void,
    result: Result<Option<T>>,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe {
        output(
            out,
            result.map(|value| value.map_or(std::ptr::null_mut(), Interface::into_raw)),
        )
    }
}

impl SimpleAbi_Impl for Provider_Impl {
    unsafe fn ProviderOptions(&self, out: *mut ProviderOptions) -> HRESULT {
        unsafe {
            output(
                out,
                Ok(ProviderOptions_ServerSideProvider | ProviderOptions_ProviderOwnsSetFocus),
            )
        }
    }
    unsafe fn GetPatternProvider(&self, id: UIA_PATTERN_ID, out: *mut *mut c_void) -> HRESULT {
        let result = self.read(|s, node| {
            let p = provider(&self.shared, self.id);
            match (id, node.map(|n| &n.kind)) {
                (UIA_InvokePatternId, Some(Kind::Button)) => {
                    p.cast::<IInvokeProvider>()?.cast().map(Some)
                }
                (UIA_TogglePatternId, Some(Kind::Toggle(_))) => {
                    p.cast::<IToggleProvider>()?.cast().map(Some)
                }
                (UIA_RangeValuePatternId, Some(Kind::Range { .. })) => {
                    p.cast::<IRangeValueProvider>()?.cast().map(Some)
                }
                (UIA_SelectionItemPatternId, Some(Kind::Item(_))) => {
                    p.cast::<ISelectionItemProvider>()?.cast().map(Some)
                }
                (UIA_SelectionPatternId, None)
                    if s.nodes
                        .iter()
                        .any(|node| matches!(node.kind, Kind::Item(_))) =>
                {
                    p.cast::<ISelectionProvider>()?.cast().map(Some)
                }
                _ => Ok(None::<IUnknown>),
            }
        });
        unsafe { interface_output(out, result) }
    }
    unsafe fn GetPropertyValue(&self, id: UIA_PROPERTY_ID, out: *mut VARIANT) -> HRESULT {
        unsafe { output(out, self.property(id)) }
    }
    unsafe fn HostRawElementProvider(&self, out: *mut *mut c_void) -> HRESULT {
        let result = self.read(|s, _| Ok(s.hwnd)).and_then(|hwnd| {
            if self.id == 0 {
                unsafe { UiaHostProviderFromHwnd(HWND(hwnd as *mut _)) }.map(Some)
            } else {
                Ok(None)
            }
        });
        unsafe { interface_output(out, result) }
    }
}

impl FragmentAbi_Impl for Provider_Impl {
    unsafe fn Navigate(&self, direction: NavigateDirection, out: *mut *mut c_void) -> HRESULT {
        let result = self.read(|s, _| {
            let at = s.nodes.iter().position(|node| node.id == self.id);
            let id = match direction {
                NavigateDirection_Parent if self.id != 0 => Some(0),
                NavigateDirection_FirstChild if self.id == 0 => s.nodes.first().map(|n| n.id),
                NavigateDirection_LastChild if self.id == 0 => s.nodes.last().map(|n| n.id),
                NavigateDirection_NextSibling => at.and_then(|i| s.nodes.get(i + 1)).map(|n| n.id),
                NavigateDirection_PreviousSibling => at
                    .and_then(|i| i.checked_sub(1))
                    .and_then(|i| s.nodes.get(i))
                    .map(|n| n.id),
                _ => None,
            };
            id.map(|id| provider(&self.shared, id).cast::<IRawElementProviderFragment>())
                .transpose()
        });
        unsafe { interface_output(out, result) }
    }
    unsafe fn GetRuntimeId(&self, out: *mut *mut SAFEARRAY) -> HRESULT {
        if out.is_null() {
            return E_POINTER;
        }
        let result = self.read(|_, _| {
            if self.id == 0 {
                return Ok(std::ptr::null_mut());
            }
            let array = unsafe { SafeArrayCreateVector(VT_I4, 0, 3) };
            if array.is_null() {
                return Err(E_OUTOFMEMORY.into());
            }
            for (index, value) in [UiaAppendRuntimeId, self.shared.generation, self.id]
                .into_iter()
                .enumerate()
            {
                if let Err(error) = unsafe {
                    SafeArrayPutElement(
                        array,
                        &i32::try_from(index).unwrap_or(0),
                        (&raw const value).cast(),
                    )
                } {
                    let _ = unsafe { SafeArrayDestroy(array) };
                    return Err(error);
                }
            }
            Ok(array)
        });
        unsafe { output(out, result) }
    }
    unsafe fn BoundingRectangle(&self, out: *mut UiaRect) -> HRESULT {
        let result = self.read(|s, node| {
            let r = node.map_or(s.root_rect, |n| n.rect);
            if r.right <= r.left || r.bottom <= r.top {
                return Ok(UiaRect::default());
            }
            Ok(UiaRect {
                left: f64::from(r.left + s.origin.x),
                top: f64::from(r.top + s.origin.y),
                width: f64::from((r.right - r.left).max(0)),
                height: f64::from((r.bottom - r.top).max(0)),
            })
        });
        unsafe { output(out, result) }
    }
    unsafe fn GetEmbeddedFragmentRoots(&self, out: *mut *mut SAFEARRAY) -> HRESULT {
        unsafe { output(out, self.read(|_, _| Ok(std::ptr::null_mut()))) }
    }
    unsafe fn SetFocus(&self) -> HRESULT {
        self.queue(Action::Focus(self.id))
            .map_or_else(|e| e.code(), |()| S_OK)
    }
    unsafe fn FragmentRoot(&self, out: *mut *mut c_void) -> HRESULT {
        unsafe {
            interface_output(
                out,
                self.read(|_, _| {
                    provider(&self.shared, 0)
                        .cast::<IRawElementProviderFragmentRoot>()
                        .map(Some)
                }),
            )
        }
    }
}

impl RootAbi_Impl for Provider_Impl {
    unsafe fn ElementProviderFromPoint(&self, x: f64, y: f64, out: *mut *mut c_void) -> HRESULT {
        let result = self.read(|s, _| {
            let (x, y) = (x - f64::from(s.origin.x), y - f64::from(s.origin.y));
            let r = s.root_rect;
            if x < f64::from(r.left)
                || x >= f64::from(r.right)
                || y < f64::from(r.top)
                || y >= f64::from(r.bottom)
            {
                return Ok(None);
            }
            let id = s
                .nodes
                .iter()
                .rev()
                .find(|n| {
                    x >= f64::from(n.rect.left)
                        && x < f64::from(n.rect.right)
                        && y >= f64::from(n.rect.top)
                        && y < f64::from(n.rect.bottom)
                })
                .map_or(0, |n| n.id);
            provider(&self.shared, id)
                .cast::<IRawElementProviderFragment>()
                .map(Some)
        });
        unsafe { interface_output(out, result) }
    }
    unsafe fn GetFocus(&self, out: *mut *mut c_void) -> HRESULT {
        unsafe {
            interface_output(
                out,
                self.read(|s, _| {
                    s.focused
                        .map(|id| provider(&self.shared, id).cast::<IRawElementProviderFragment>())
                        .transpose()
                }),
            )
        }
    }
}

impl IInvokeProvider_Impl for Provider_Impl {
    fn Invoke(&self) -> Result<()> {
        self.read(|_, node| {
            if node.is_some_and(|n| n.kind == Kind::Button) {
                Ok(())
            } else {
                Err(failure(UIA_E_NOTSUPPORTED))
            }
        })?;
        self.queue(Action::Invoke(self.id))
    }
}
impl IToggleProvider_Impl for Provider_Impl {
    fn Toggle(&self) -> Result<()> {
        self.ToggleState()?;
        self.queue(Action::Invoke(self.id))
    }
    fn ToggleState(&self) -> Result<ToggleState> {
        self.read(|_, node| match node.map(|n| &n.kind) {
            Some(Kind::Toggle(true)) => Ok(ToggleState_On),
            Some(Kind::Toggle(false)) => Ok(ToggleState_Off),
            _ => Err(failure(UIA_E_NOTSUPPORTED)),
        })
    }
}
impl IRangeValueProvider_Impl for Provider_Impl {
    fn SetValue(&self, val: f64) -> Result<()> {
        let (_, min, max) = self.range()?;
        if !val.is_finite() || !(min..=max).contains(&val) {
            return Err(E_INVALIDARG.into());
        }
        self.queue(Action::SetRange(self.id, val))
    }
    fn Value(&self) -> Result<f64> {
        self.range().map(|(value, _, _)| value)
    }
    fn IsReadOnly(&self) -> Result<BOOL> {
        self.range()?;
        Ok(false.into())
    }
    fn Maximum(&self) -> Result<f64> {
        self.range().map(|(_, _, maximum)| maximum)
    }
    fn Minimum(&self) -> Result<f64> {
        self.range().map(|(_, minimum, _)| minimum)
    }
    fn LargeChange(&self) -> Result<f64> {
        self.range()?;
        Ok(5.0)
    }
    fn SmallChange(&self) -> Result<f64> {
        self.range()?;
        Ok(1.0 / 60.0)
    }
}

impl ISelectionItemProvider_Impl for Provider_Impl {
    fn Select(&self) -> Result<()> {
        let _ = self.IsSelected()?;
        self.queue(Action::Select(self.id))
    }
    fn AddToSelection(&self) -> Result<()> {
        self.read(|s, _| {
            if s.nodes
                .iter()
                .any(|node| node.id != self.id && node.kind == Kind::Item(true))
            {
                Err(failure(UIA_E_INVALIDOPERATION))
            } else {
                Ok(())
            }
        })?;
        self.Select()
    }
    fn RemoveFromSelection(&self) -> Result<()> {
        if self.IsSelected()?.as_bool() {
            self.queue(Action::ClearSelection(self.id))?;
        }
        Ok(())
    }
    fn IsSelected(&self) -> Result<BOOL> {
        self.read(|_, node| match node.map(|n| &n.kind) {
            Some(Kind::Item(selected)) => Ok((*selected).into()),
            _ => Err(failure(UIA_E_NOTSUPPORTED)),
        })
    }
    fn SelectionContainer(&self) -> Result<IRawElementProviderSimple> {
        let _ = self.IsSelected()?;
        provider(&self.shared, 0).cast()
    }
}

impl ISelectionProvider_Impl for Provider_Impl {
    fn GetSelection(&self) -> Result<*mut SAFEARRAY> {
        let selected = self.read(|s, _| {
            if self.id != 0 {
                return Err(failure(UIA_E_NOTSUPPORTED));
            }
            Ok(s.nodes
                .iter()
                .filter(|n| n.kind == Kind::Item(true))
                .map(|n| n.id)
                .collect::<Vec<_>>())
        })?;
        let array = unsafe {
            SafeArrayCreateVector(
                VT_UNKNOWN,
                0,
                u32::try_from(selected.len()).map_err(|_| Error::from(E_OUTOFMEMORY))?,
            )
        };
        if array.is_null() {
            return Err(E_OUTOFMEMORY.into());
        }
        for (index, id) in selected.into_iter().enumerate() {
            let p = provider(&self.shared, id);
            if let Err(error) = unsafe {
                SafeArrayPutElement(
                    array,
                    &i32::try_from(index).unwrap_or(0),
                    p.as_raw().cast_const(),
                )
            } {
                let _ = unsafe { SafeArrayDestroy(array) };
                return Err(error);
            }
        }
        Ok(array)
    }
    fn CanSelectMultiple(&self) -> Result<BOOL> {
        self.read(|_, _| Ok(false.into()))
    }
    fn IsSelectionRequired(&self) -> Result<BOOL> {
        self.read(|_, _| Ok(false.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> AccessibilityHost {
        let host = AccessibilityHost::default();
        let rect = RECT {
            left: 10,
            top: 20,
            right: 60,
            bottom: 50,
        };
        *host.0.lock() = Snapshot {
            alive: true,
            enabled: true,
            name: "测试编辑器".into(),
            root_rect: RECT {
                right: 800,
                bottom: 600,
                ..RECT::default()
            },
            origin: POINT { x: 200, y: 100 },
            nodes: vec![
                Node::button(1, "播放视频", "Space", rect, true),
                Node {
                    id: 2,
                    name: "应用镜头".into(),
                    help: String::new(),
                    rect,
                    enabled: true,
                    kind: Kind::Toggle(true),
                },
                Node {
                    id: PLAYHEAD_ID,
                    name: "播放位置（秒）".into(),
                    help: String::new(),
                    rect,
                    enabled: true,
                    kind: Kind::Range {
                        value: 1.25,
                        minimum: 0.0,
                        maximum: 12.0,
                    },
                },
            ],
            ..Snapshot::default()
        };
        host
    }

    #[test]
    fn abi_matches_windows_interfaces_and_null_outputs_are_initialized() {
        use windows::Win32::UI::Accessibility::{
            IRawElementProviderFragment_Vtbl, IRawElementProviderFragmentRoot_Vtbl,
            IRawElementProviderSimple_Vtbl,
        };
        assert_eq!(SimpleAbi::IID, IRawElementProviderSimple::IID);
        assert_eq!(FragmentAbi::IID, IRawElementProviderFragment::IID);
        assert_eq!(RootAbi::IID, IRawElementProviderFragmentRoot::IID);
        assert_eq!(
            std::mem::size_of::<SimpleAbi_Vtbl>(),
            std::mem::size_of::<IRawElementProviderSimple_Vtbl>()
        );
        assert_eq!(
            std::mem::size_of::<FragmentAbi_Vtbl>(),
            std::mem::size_of::<IRawElementProviderFragment_Vtbl>()
        );
        assert_eq!(
            std::mem::size_of::<RootAbi_Vtbl>(),
            std::mem::size_of::<IRawElementProviderFragmentRoot_Vtbl>()
        );
        let host = fixture();
        let p = provider(&host.0, 1);
        let fragment: FragmentAbi = p.cast().unwrap();
        let mut out = std::ptr::dangling_mut::<c_void>();
        assert_eq!(
            unsafe { p.GetPatternProvider(UIA_TogglePatternId, &raw mut out) },
            S_OK
        );
        assert!(out.is_null());
        out = std::ptr::dangling_mut();
        assert_eq!(unsafe { p.HostRawElementProvider(&raw mut out) }, S_OK);
        assert!(out.is_null());
        out = std::ptr::dangling_mut();
        assert_eq!(
            unsafe { fragment.Navigate(NavigateDirection_PreviousSibling, &raw mut out) },
            S_OK
        );
        assert!(out.is_null());
        let root: RootAbi = provider(&host.0, 0).cast().unwrap();
        out = std::ptr::dangling_mut();
        assert_eq!(unsafe { root.GetFocus(&raw mut out) }, S_OK);
        assert!(out.is_null());
        assert_eq!(
            unsafe { p.GetPatternProvider(UIA_InvokePatternId, std::ptr::null_mut()) },
            E_POINTER
        );
    }

    #[test]
    fn chinese_properties_patterns_navigation_and_screen_coordinates() {
        let host = fixture();
        let p: IRawElementProviderSimple = provider(&host.0, 1).cast().unwrap();
        assert_eq!(
            unsafe { p.GetPropertyValue(UIA_NamePropertyId) }
                .unwrap()
                .to_string(),
            "播放视频"
        );
        assert!(
            bool::try_from(
                &unsafe { p.GetPropertyValue(UIA_IsInvokePatternAvailablePropertyId) }.unwrap()
            )
            .unwrap()
        );
        let invoke = unsafe { p.GetPatternProvider(UIA_InvokePatternId) }.unwrap();
        assert!(invoke.cast::<IInvokeProvider>().is_ok());
        let fragment: IRawElementProviderFragment = p.cast().unwrap();
        let rect = unsafe { fragment.BoundingRectangle() }.unwrap();
        assert_eq!(
            (rect.left, rect.top, rect.width, rect.height),
            (210.0, 120.0, 50.0, 30.0)
        );
        let next = unsafe { fragment.Navigate(NavigateDirection_NextSibling) }.unwrap();
        let toggle: IToggleProvider = next.cast().unwrap();
        assert_eq!(unsafe { toggle.ToggleState() }.unwrap(), ToggleState_On);
        let slider: IRangeValueProvider = provider(&host.0, PLAYHEAD_ID).cast().unwrap();
        assert_eq!(unsafe { slider.Value() }.unwrap(), 1.25);
        assert_eq!(unsafe { slider.Maximum() }.unwrap(), 12.0);
        for invalid in [f64::NAN, f64::INFINITY, -1.0, 12.1] {
            assert_eq!(
                unsafe { slider.SetValue(invalid) }.unwrap_err().code(),
                E_INVALIDARG
            );
        }
    }

    #[test]
    fn close_and_removed_nodes_invalidate_all_retained_client_references() {
        let host = fixture();
        let p: IRawElementProviderSimple = provider(&host.0, 1).cast().unwrap();
        host.0.lock().nodes.retain(|node| node.id != 1);
        assert_eq!(
            unsafe { p.GetPropertyValue(UIA_NamePropertyId) }
                .unwrap_err()
                .code(),
            failure(UIA_E_ELEMENTNOTAVAILABLE).code()
        );
        let slider: IRangeValueProvider = provider(&host.0, PLAYHEAD_ID).cast().unwrap();
        drop(host);
        assert_eq!(
            unsafe { slider.Value() }.unwrap_err().code(),
            failure(UIA_E_ELEMENTNOTAVAILABLE).code()
        );
        assert_eq!(
            unsafe { slider.SetValue(2.0) }.unwrap_err().code(),
            failure(UIA_E_ELEMENTNOTAVAILABLE).code()
        );
    }

    #[test]
    fn detaching_a_page_never_revives_retained_providers_after_ids_are_reused() {
        let mut host = fixture();
        let previous = Arc::clone(&host.0);
        let retained: IRawElementProviderSimple = provider(&previous, 1).cast().unwrap();
        let invoke = host.review_invoke_provider(1).unwrap();
        let nodes = previous.lock().nodes.clone();
        {
            let mut state = previous.lock();
            state.pending.push_back(Action::Invoke(1));
            state.invoked.push(1);
            state.action_posted = true;
            state.notify_pending = true;
        }
        host.detach();
        assert_ne!(host.0.generation, previous.generation);
        assert!(previous.lock().pending.is_empty());
        assert!(previous.lock().invoked.is_empty());
        assert!(!previous.lock().notify_pending);
        // The replacement page may reuse the same HWND and local control IDs.
        // Old COM clients must still see an unavailable element, not the new action.
        {
            let mut replacement = host.0.lock();
            replacement.alive = true;
            replacement.enabled = true;
            replacement.nodes = nodes;
            replacement.nodes[0].name = "新页面的操作".into();
        }
        assert_eq!(
            unsafe { retained.GetPropertyValue(UIA_NamePropertyId) }
                .unwrap_err()
                .code(),
            failure(UIA_E_ELEMENTNOTAVAILABLE).code()
        );
        assert_eq!(
            unsafe { invoke.Invoke() }.unwrap_err().code(),
            failure(UIA_E_ELEMENTNOTAVAILABLE).code()
        );
        let current: IRawElementProviderSimple = provider(&host.0, 1).cast().unwrap();
        assert_eq!(
            unsafe { current.GetPropertyValue(UIA_NamePropertyId) }
                .unwrap()
                .to_string(),
            "新页面的操作"
        );
        assert!(host.take_actions().is_empty());
    }

    #[test]
    fn runtime_ids_are_stable_and_differ_across_window_generations() {
        use windows::Win32::System::Ole::SafeArrayGetElement;
        fn runtime(shared: &Arc<Shared>, id: u32) -> Vec<i32> {
            let p: IRawElementProviderFragment = provider(shared, id).cast().unwrap();
            let array = unsafe { p.GetRuntimeId() }.unwrap();
            let values = (0..3)
                .map(|index| {
                    let mut value = 0_i32;
                    unsafe {
                        SafeArrayGetElement(array, &raw const index, (&raw mut value).cast())
                            .unwrap();
                    }
                    value
                })
                .collect();
            unsafe {
                SafeArrayDestroy(array).unwrap();
            }
            values
        }
        let first = fixture();
        let second = fixture();
        assert_eq!(runtime(&first.0, 1), runtime(&first.0, 1));
        assert_ne!(runtime(&first.0, 1), runtime(&first.0, 2));
        assert_ne!(runtime(&first.0, 1), runtime(&second.0, 1));
    }

    #[test]
    fn range_requests_coalesce_without_reordering_discrete_edit_commands() {
        let mut pending = VecDeque::new();
        for n in 0..10_000 {
            enqueue(&mut pending, Action::SetRange(PLAYHEAD_ID, f64::from(n))).unwrap();
        }
        assert_eq!(pending.len(), 1);
        enqueue(&mut pending, Action::Invoke(1)).unwrap();
        enqueue(&mut pending, Action::SetRange(PLAYHEAD_ID, 2.0)).unwrap();
        assert_eq!(
            pending.into_iter().collect::<Vec<_>>(),
            vec![
                Action::SetRange(PLAYHEAD_ID, 9999.0),
                Action::Invoke(1),
                Action::SetRange(PLAYHEAD_ID, 2.0)
            ]
        );
    }

    #[test]
    fn discrete_command_queue_is_bounded_and_does_not_silently_drop() {
        let mut pending = VecDeque::new();
        for _ in 0..MAX_PENDING {
            enqueue(&mut pending, Action::Invoke(1)).unwrap();
        }
        assert_eq!(
            enqueue(&mut pending, Action::Invoke(2)).unwrap_err().code(),
            failure(UIA_E_INVALIDOPERATION).code()
        );
        assert_eq!(pending.len(), MAX_PENDING);
        assert!(pending.iter().all(|action| *action == Action::Invoke(1)));
    }

    #[test]
    fn selection_returns_owned_interfaces_and_stable_object_ids() {
        use windows::Win32::System::Ole::{SafeArrayGetElement, SafeArrayGetUBound};
        let host = fixture();
        let id = host.stable_id("video:source-one".into());
        assert_eq!(id, host.stable_id("video:source-one".into()));
        assert_ne!(id, host.stable_id("camera:source-one".into()));
        assert_eq!(host.key(id).as_deref(), Some("video:source-one"));
        host.0.lock().nodes.push(Node {
            id,
            name: "视频片段 1".into(),
            help: String::new(),
            rect: RECT {
                right: 100,
                bottom: 20,
                ..RECT::default()
            },
            enabled: true,
            kind: Kind::Item(true),
        });
        let item: ISelectionItemProvider = provider(&host.0, id).cast().unwrap();
        assert!(unsafe { item.IsSelected() }.unwrap().as_bool());
        let root: ISelectionProvider = provider(&host.0, 0).cast().unwrap();
        assert!(!unsafe { root.CanSelectMultiple() }.unwrap().as_bool());
        let array = unsafe { root.GetSelection() }.unwrap();
        assert_eq!(unsafe { SafeArrayGetUBound(array, 1) }.unwrap(), 0);
        let mut raw = std::ptr::null_mut::<c_void>();
        unsafe {
            SafeArrayGetElement(array, &0, (&raw mut raw).cast()).unwrap();
        }
        assert!(!raw.is_null());
        let p = unsafe { IUnknown::from_raw(raw) }
            .cast::<IRawElementProviderSimple>()
            .unwrap();
        assert_eq!(
            unsafe { p.GetPropertyValue(UIA_NamePropertyId) }
                .unwrap()
                .to_string(),
            "视频片段 1"
        );
        unsafe {
            SafeArrayDestroy(array).unwrap();
        }
    }

    #[test]
    fn property_notifications_track_names_enabled_values_and_selection() {
        let host = fixture();
        let before = host.0.lock().nodes.clone();
        let mut after = before.clone();
        after[0].name = "暂停视频".into();
        after[1].kind = Kind::Toggle(false);
        after[2].kind = Kind::Range {
            value: 4.0,
            minimum: 0.0,
            maximum: 12.0,
        };
        let changes = property_changes(&before, &after, true, false);
        assert_eq!(
            changes
                .iter()
                .filter(|change| change.1 == UIA_IsEnabledPropertyId)
                .count(),
            3
        );
        for property in [
            UIA_NamePropertyId,
            UIA_ToggleToggleStatePropertyId,
            UIA_RangeValueValuePropertyId,
        ] {
            assert!(changes.iter().any(|change| change.1 == property));
        }
        assert!(property_changes(&after, &after, true, true).is_empty());
    }

    #[test]
    fn concurrent_client_reads_do_not_borrow_owner_state() {
        let host = fixture();
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let shared = Arc::clone(&host.0);
                std::thread::spawn(move || {
                    let p = Provider { shared, id: 1 };
                    for _ in 0..500 {
                        assert_eq!(
                            p.property(UIA_NamePropertyId).unwrap().to_string(),
                            "播放视频"
                        );
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
    }
}
