//! Menu-bar tool that keeps one virtual screen on a Mac at a chosen size and density.
//!
//! The display is on from launch; **Enabled** in the menu turns it off and on.
//! **Settings…** changes its size and density, which are saved in the user
//! defaults. The display belongs to this process: Quit takes it away, and so
//! does a crash.
//!
//! An argument of the form `WIDTHxHEIGHT@SCALE` (points, scale 1 or 2) replaces
//! the saved size for this run.

mod display;

use std::cell::{Cell, OnceCell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSApplicationActivationPolicy,
    NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu, NSMenuDelegate, NSMenuItem,
    NSPopUpButton, NSStatusBar, NSStatusItem, NSTextField, NSVariableStatusItemLength, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSUserDefaults};

use display::{Spec, VirtualDisplay};

struct Ivars {
    /// `Some` while enabled.
    display: RefCell<Option<VirtualDisplay>>,
    /// The size and density the display is made at.
    spec: Cell<Spec>,
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
        #[unsafe(method(toggleEnabled:))]
        fn toggle_enabled(&self, _sender: Option<&AnyObject>) {
            if self.ivars().display.borrow_mut().take().is_some() {
                return;
            }
            if let Err(err) = self.enable() {
                alert(MainThreadMarker::from(self), "Could not enable the display", &format!("{err:#}"));
            }
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            let mtm = MainThreadMarker::from(self);
            let mut draft = Draft::from(self.ivars().spec.get());
            loop {
                let Some(edited) = dialog(mtm, &draft) else {
                    return;
                };
                draft = edited.clone();
                match edited.parse().and_then(|spec| self.apply(spec)) {
                    Ok(()) => return,
                    Err(err) => alert(mtm, "Could not apply the settings", &format!("{err:#}")),
                }
            }
        }
    }
);

impl Controller {
    fn new(mtm: MainThreadMarker, spec: Spec) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            display: RefCell::new(None),
            spec: Cell::new(spec),
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

    /// Make the display at the current spec. Does nothing if it is already on.
    fn enable(&self) -> anyhow::Result<()> {
        let mut display = self.ivars().display.borrow_mut();
        if display.is_none() {
            *display = Some(VirtualDisplay::create(self.ivars().spec.get())?);
        }
        Ok(())
    }

    /// Save `spec` as the setting and, if the display is on, remake it at `spec`.
    ///
    /// The descriptor fixes the framebuffer size, so a new size or density takes
    /// a new display. It has the same identity as the old one, so the old one
    /// goes first; if the new one fails, the old one is put back.
    fn apply(&self, spec: Spec) -> anyhow::Result<()> {
        let old = self.ivars().spec.get();
        let mut display = self.ivars().display.borrow_mut();
        if display.is_some() && spec != old {
            display.take();
            match VirtualDisplay::create(spec) {
                Ok(new) => *display = Some(new),
                Err(err) => {
                    *display = VirtualDisplay::create(old)
                        .inspect_err(|e| eprintln!("vdisplay: could not restore {old:?}: {e:#}"))
                        .ok();
                    return Err(err);
                }
            }
        }
        self.ivars().spec.set(spec);
        save(spec);
        Ok(())
    }

    fn rebuild(&self, menu: &NSMenu) {
        let mtm = MainThreadMarker::from(self);
        menu.removeAllItems();
        let spec = self.ivars().spec.get();
        let display = self.ivars().display.borrow();
        let status = match &*display {
            Some(d) => d.status(),
            None => "disabled".to_owned(),
        };
        let title = format!(
            "{}x{} pt @{}x ({status})",
            spec.width,
            spec.height,
            spec.scale()
        );
        menu.addItem(&self.item(mtm, &title, None));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let enabled = self.item(mtm, "Enabled", Some(sel!(toggleEnabled:)));
        enabled.setState(if display.is_some() {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        menu.addItem(&enabled);
        menu.addItem(&self.item(mtm, "Settings…", Some(sel!(openSettings:))));
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
        item.setEnabled(action.is_some());
        item
    }
}

const WIDTH_KEY: &str = "width";
const HEIGHT_KEY: &str = "height";
const HIDPI_KEY: &str = "hidpi";

/// The saved spec, or the default when none is saved or it no longer validates.
fn load() -> Spec {
    let defaults = NSUserDefaults::standardUserDefaults();
    if defaults
        .objectForKey(&NSString::from_str(WIDTH_KEY))
        .is_none()
    {
        return Spec::DEFAULT;
    }
    let number = |key: &str| u32::try_from(defaults.integerForKey(&NSString::from_str(key))).ok();
    let spec = (|| {
        Some(Spec {
            width: number(WIDTH_KEY)?,
            height: number(HEIGHT_KEY)?,
            hidpi: defaults.boolForKey(&NSString::from_str(HIDPI_KEY)),
        })
    })();
    match spec {
        Some(spec) if spec.validate().is_ok() => spec,
        _ => {
            eprintln!("vdisplay: saved settings {spec:?} are not usable; using the default");
            Spec::DEFAULT
        }
    }
}

fn save(spec: Spec) {
    let defaults = NSUserDefaults::standardUserDefaults();
    defaults.setInteger_forKey(spec.width as isize, &NSString::from_str(WIDTH_KEY));
    defaults.setInteger_forKey(spec.height as isize, &NSString::from_str(HEIGHT_KEY));
    defaults.setBool_forKey(spec.hidpi, &NSString::from_str(HIDPI_KEY));
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
            s.trim().parse().map_err(|_| {
                anyhow::anyhow!("{name} {:?} is not a whole number of points", s.trim())
            })
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

/// The Settings dialog. `None` when cancelled.
fn dialog(mtm: MainThreadMarker, draft: &Draft) -> Option<Draft> {
    let row = |y: f64| NSRect::new(NSPoint::new(120.0, y), NSSize::new(140.0, 24.0));
    let label = |text: &str, y: f64| {
        let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
        l.setFrame(NSRect::new(
            NSPoint::new(0.0, y + 3.0),
            NSSize::new(115.0, 18.0),
        ));
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
    alert.setMessageText(&NSString::from_str("Virtual display settings"));
    alert.setInformativeText(&NSString::from_str(
        "Size is in points. At 2x (HiDPI) the framebuffer has twice the pixels on each axis. \
         If the display is on, applying remakes it at the new size.",
    ));
    alert.addButtonWithTitle(&NSString::from_str("Apply"));
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let spec = match args.as_slice() {
        [] => load(),
        [arg] => Spec::from_arg(arg).unwrap_or_else(|err| {
            eprintln!("vdisplay: {arg}: {err:#}");
            std::process::exit(2);
        }),
        _ => {
            eprintln!("vdisplay: makes only one display; got {} specs", args.len());
            std::process::exit(2);
        }
    };
    let controller = Controller::new(mtm, spec);
    controller.install(mtm);
    if let Err(err) = controller.enable() {
        eprintln!("vdisplay: {err:#}");
        alert(mtm, "Could not enable the display", &format!("{err:#}"));
    }
    app.run();
}
