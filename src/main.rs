//! Menu-bar tool that adds virtual screens to a Mac at a chosen size and density.
//!
//! Every display belongs to this process: Remove or Quit takes it away, and so
//! does a crash. Nothing is saved between runs.
//!
//! Arguments of the form `WIDTHxHEIGHT@SCALE` (points, scale 1 or 2) create
//! displays at launch.

mod display;

use std::cell::{OnceCell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSApplicationActivationPolicy, NSImage,
    NSMenu, NSMenuDelegate, NSMenuItem, NSPopUpButton, NSStatusBar, NSStatusItem, NSTextField,
    NSVariableStatusItemLength, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use display::{Spec, VirtualDisplay};

struct Ivars {
    displays: RefCell<Vec<VirtualDisplay>>,
    /// What the dialog opens with: the last spec asked for.
    last: RefCell<Spec>,
    item: OnceCell<Retained<NSStatusItem>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `Controller` does not
    // implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "VDisplayController"]
    #[ivars = Ivars]
    struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl NSMenuDelegate for Controller {
        /// Rebuilt on open so the sizes shown are the WindowServer's current ones.
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            self.rebuild(menu);
        }
    }

    impl Controller {
        #[unsafe(method(addDisplay:))]
        fn add_display(&self, _sender: Option<&AnyObject>) {
            let mtm = MainThreadMarker::from(self);
            let mut draft = Draft::from(*self.ivars().last.borrow());
            loop {
                let Some(edited) = dialog(mtm, &draft) else {
                    return;
                };
                draft = edited.clone();
                match edited.parse().and_then(|spec| self.create(spec)) {
                    Ok(()) => return,
                    Err(err) => alert(mtm, "Could not add the display", &format!("{err:#}")),
                }
            }
        }

        #[unsafe(method(removeDisplay:))]
        fn remove_display(&self, sender: Option<&AnyObject>) {
            let Some(sender) = sender else { return };
            let id: isize = unsafe { msg_send![sender, tag] };
            self.ivars()
                .displays
                .borrow_mut()
                .retain(|d| d.id as isize != id);
        }

        #[unsafe(method(removeAll:))]
        fn remove_all(&self, _sender: Option<&AnyObject>) {
            self.ivars().displays.borrow_mut().clear();
        }
    }
);

impl Controller {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            displays: RefCell::new(Vec::new()),
            last: RefCell::new(Spec {
                width: 1920,
                height: 1080,
                hidpi: true,
            }),
            item: OnceCell::new(),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn install(&self, mtm: MainThreadMarker) {
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        if let Some(button) = item.button(mtm) {
            let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str("display.2"),
                Some(&NSString::from_str("vdisplay")),
            );
            match image {
                Some(image) => {
                    image.setTemplate(true);
                    button.setImage(Some(&image));
                }
                None => button.setTitle(&NSString::from_str("vd")),
            }
        }
        let menu = NSMenu::new(mtm);
        menu.setDelegate(Some(ProtocolObject::from_ref(self)));
        item.setMenu(Some(&menu));
        let _ = self.ivars().item.set(item);
    }

    fn create(&self, spec: Spec) -> anyhow::Result<()> {
        let mut displays = self.ivars().displays.borrow_mut();
        let slot = (1..)
            .find(|s| displays.iter().all(|d| d.slot != *s))
            .expect("a free slot");
        displays.push(VirtualDisplay::create(spec, slot)?);
        *self.ivars().last.borrow_mut() = spec;
        Ok(())
    }

    fn rebuild(&self, menu: &NSMenu) {
        let mtm = MainThreadMarker::from(self);
        menu.removeAllItems();
        let displays = self.ivars().displays.borrow();
        if displays.is_empty() {
            menu.addItem(&self.item(mtm, "No virtual displays", None, 0));
        }
        for d in displays.iter() {
            let title = format!(
                "Remove display {}: {}x{} pt @{}x ({})",
                d.id,
                d.spec.width,
                d.spec.height,
                d.spec.scale(),
                d.status()
            );
            menu.addItem(&self.item(mtm, &title, Some(sel!(removeDisplay:)), d.id as isize));
        }
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&self.item(mtm, "Add Display…", Some(sel!(addDisplay:)), 0));
        if !displays.is_empty() {
            menu.addItem(&self.item(mtm, "Remove All", Some(sel!(removeAll:)), 0));
        }
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let quit = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str("Quit vdisplay"),
                Some(sel!(terminate:)),
                &NSString::from_str("q"),
            )
        };
        menu.addItem(&quit);
    }

    /// A menu item aimed at this controller; disabled when it has no action.
    fn item(
        &self,
        mtm: MainThreadMarker,
        title: &str,
        action: Option<Sel>,
        tag: isize,
    ) -> Retained<NSMenuItem> {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                action,
                &NSString::from_str(""),
            )
        };
        let target: &AnyObject = self;
        unsafe { item.setTarget(Some(target)) };
        item.setTag(tag);
        item.setEnabled(action.is_some());
        item
    }
}

/// What the dialog holds, as typed.
#[derive(Clone)]
struct Draft {
    width: String,
    height: String,
    hidpi: bool,
}

impl From<Spec> for Draft {
    fn from(spec: Spec) -> Self {
        Self {
            width: spec.width.to_string(),
            height: spec.height.to_string(),
            hidpi: spec.hidpi,
        }
    }
}

impl Draft {
    fn parse(&self) -> anyhow::Result<Spec> {
        let number = |name: &str, s: &str| -> anyhow::Result<u32> {
            s.trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("{name} {:?} is not a whole number of points", s.trim()))
        };
        let spec = Spec {
            width: number("width", &self.width)?,
            height: number("height", &self.height)?,
            hidpi: self.hidpi,
        };
        spec.validate()?;
        Ok(spec)
    }
}

/// The Add Display dialog. `None` when cancelled.
fn dialog(mtm: MainThreadMarker, draft: &Draft) -> Option<Draft> {
    let row = |y: f64| NSRect::new(NSPoint::new(120.0, y), NSSize::new(140.0, 24.0));
    let label = |text: &str, y: f64| {
        let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
        l.setFrame(NSRect::new(NSPoint::new(0.0, y + 3.0), NSSize::new(115.0, 18.0)));
        l
    };
    let field = |value: &str, y: f64| {
        let f = NSTextField::initWithFrame(NSTextField::alloc(mtm), row(y));
        f.setStringValue(&NSString::from_str(value));
        f
    };

    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(260.0, 92.0)),
    );
    let width = field(&draft.width, 64.0);
    let height = field(&draft.height, 34.0);
    let scale = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), row(2.0), false);
    scale.addItemWithTitle(&NSString::from_str("2x (HiDPI)"));
    scale.addItemWithTitle(&NSString::from_str("1x"));
    scale.selectItemAtIndex(if draft.hidpi { 0 } else { 1 });
    view.addSubview(&label("Width (points)", 64.0));
    view.addSubview(&width);
    view.addSubview(&label("Height (points)", 34.0));
    view.addSubview(&height);
    view.addSubview(&label("Density", 2.0));
    view.addSubview(&scale);

    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str("Add a virtual display"));
    alert.setInformativeText(&NSString::from_str(
        "Size is in points. At 2x the framebuffer has twice the pixels on each axis.",
    ));
    alert.addButtonWithTitle(&NSString::from_str("Add"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    alert.setAccessoryView(Some(&view));
    let first: &NSView = &width;
    alert.window().setInitialFirstResponder(Some(first));

    activate(mtm);
    if alert.runModal() != NSAlertFirstButtonReturn {
        return None;
    }
    Some(Draft {
        width: width.stringValue().to_string(),
        height: height.stringValue().to_string(),
        hidpi: scale.indexOfSelectedItem() == 0,
    })
}

fn alert(mtm: MainThreadMarker, title: &str, text: &str) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(text));
    activate(mtm);
    alert.runModal();
}

/// An accessory app's modal opens behind the frontmost app unless it is activated.
fn activate(mtm: MainThreadMarker) {
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
}

fn main() {
    let mtm = MainThreadMarker::new().expect("vdisplay must start on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let controller = Controller::new(mtm);
    controller.install(mtm);
    for arg in std::env::args().skip(1) {
        if let Err(err) = Spec::from_arg(&arg).and_then(|spec| controller.create(spec)) {
            eprintln!("vdisplay: {arg}: {err:#}");
            std::process::exit(2);
        }
    }
    app.run();
}
