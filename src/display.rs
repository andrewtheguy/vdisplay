//! One screen made with the private `CGVirtualDisplay` API.
//!
//! The mode is always listed in **points**; `hiDPI` decides how many pixels sit
//! behind it. Listing a 2x mode at its pixel size instead gives a display of that
//! point size with no extra pixels.
//!
//! The descriptor's `maxPixels` is a ceiling macOS enforces by silently halving,
//! so it is set to exactly `points * scale`.

use anyhow::Context as _;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{msg_send, sel};
use objc2_core_foundation::{CGPoint, CGSize};
use objc2_core_graphics::{CGDisplayBounds, CGDisplayIsActive, CGDisplayIsOnline};
use objc2_foundation::{NSArray, NSString};

/// Vendor id every display this tool makes reports ("vd").
const VENDOR: u32 = 0x7664;

/// Physical density the descriptor claims. 2x sits near the top of the window
/// macOS treats as Retina; 1x well below it.
const DPI_2X: f64 = 250.0;
const DPI_1X: f64 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub width: u32,
    pub height: u32,
    pub hidpi: bool,
}

impl Spec {
    pub const MIN: (u32, u32) = (640, 480);
    /// Largest framebuffer edge accepted, in pixels.
    pub const MAX_PIXELS: u32 = 8192;

    pub fn scale(self) -> u32 {
        if self.hidpi { 2 } else { 1 }
    }

    pub fn pixels(self) -> (u32, u32) {
        (self.width * self.scale(), self.height * self.scale())
    }

    pub fn validate(self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.width >= Self::MIN.0 && self.height >= Self::MIN.1,
            "{}x{} points is under the {}x{} minimum",
            self.width,
            self.height,
            Self::MIN.0,
            Self::MIN.1
        );
        let pixels = self.pixels();
        anyhow::ensure!(
            pixels.0 <= Self::MAX_PIXELS && pixels.1 <= Self::MAX_PIXELS,
            "{}x{} pixels at {}x is over the {} pixel edge limit",
            pixels.0,
            pixels.1,
            self.scale(),
            Self::MAX_PIXELS
        );
        Ok(())
    }

    /// `WIDTHxHEIGHT@SCALE`, in points, with a scale of 1 or 2.
    pub fn from_arg(arg: &str) -> anyhow::Result<Self> {
        let usage = || format!("{arg:?} is not WIDTHxHEIGHT@SCALE, e.g. 1920x1080@2");
        let (size, scale) = arg.split_once('@').with_context(usage)?;
        let (w, h) = size.split_once('x').with_context(usage)?;
        let spec = Self {
            width: w.parse().with_context(usage)?,
            height: h.parse().with_context(usage)?,
            hidpi: match scale {
                "1" => false,
                "2" => true,
                _ => anyhow::bail!(usage()),
            },
        };
        spec.validate()?;
        Ok(spec)
    }

    /// Serial number for this spec.
    ///
    /// macOS files a display's arrangement — position *and* the mode, density
    /// included — against vendor, product and serial, and restores it when that
    /// identity reappears. Deriving the serial from the spec means a size or
    /// density never inherits a mode remembered for a different one, while the
    /// same spec comes back where it was left.
    fn serial(self) -> u32 {
        self.width * 100_000 + self.height * 10 + self.scale()
    }
}

/// A live virtual display. Dropping it removes the screen.
pub struct VirtualDisplay {
    _handle: Retained<AnyObject>,
    pub id: u32,
    pub spec: Spec,
    pub slot: u32,
}

impl VirtualDisplay {
    /// Create `spec` in `slot`, which becomes the product id so two displays of
    /// the same spec are still two identities.
    pub fn create(spec: Spec, slot: u32) -> anyhow::Result<Self> {
        spec.validate()?;
        let pixels = spec.pixels();
        let dpi = if spec.hidpi { DPI_2X } else { DPI_1X };
        let mm = (
            f64::from(pixels.0) / dpi * 25.4,
            f64::from(pixels.1) / dpi * 25.4,
        );
        let descriptor = descriptor(spec, slot, pixels, mm)?;

        let class = class("CGVirtualDisplay")?;
        let allocated: Allocated<AnyObject> = unsafe { msg_send![class, alloc] };
        let display: Option<Retained<AnyObject>> =
            unsafe { msg_send![allocated, initWithDescriptor: &*descriptor] };
        let display = display.context("CGVirtualDisplay initWithDescriptor: returned nil")?;
        let id: u32 = unsafe { msg_send![&*display, displayID] };
        anyhow::ensure!(id != 0, "CGVirtualDisplay returned no display id");

        let settings = settings(spec)?;
        let applied: bool = unsafe { msg_send![&*display, applySettings: &*settings] };
        anyhow::ensure!(
            applied,
            "applySettings: refused {}x{} at {}x",
            spec.width,
            spec.height,
            spec.scale()
        );
        eprintln!(
            "vdisplay: created display {id}: {}x{} points at {}x ({}x{} pixels, serial {}, slot {slot})",
            spec.width,
            spec.height,
            spec.scale(),
            pixels.0,
            pixels.1,
            spec.serial()
        );
        Ok(Self {
            _handle: display,
            id,
            spec,
            slot,
        })
    }

    /// What the WindowServer reports now: point size, and online/active.
    pub fn status(&self) -> String {
        let bounds = CGDisplayBounds(self.id);
        let mut s = format!(
            "{}x{} pt now",
            bounds.size.width as u32, bounds.size.height as u32
        );
        if !CGDisplayIsOnline(self.id) || !CGDisplayIsActive(self.id) {
            s.push_str(", offline");
        }
        s
    }
}

impl Drop for VirtualDisplay {
    fn drop(&mut self) {
        eprintln!("vdisplay: removing display {}", self.id);
    }
}

fn class(name: &str) -> anyhow::Result<&'static AnyClass> {
    let c_name = std::ffi::CString::new(name)?;
    AnyClass::get(&c_name).with_context(|| {
        format!("{name} is not in the Objective-C runtime; the private API is gone or renamed")
    })
}

fn descriptor(
    spec: Spec,
    slot: u32,
    pixels: (u32, u32),
    mm: (f64, f64),
) -> anyhow::Result<Retained<AnyObject>> {
    let class = class("CGVirtualDisplayDescriptor")?;
    let descriptor: Retained<AnyObject> = unsafe { msg_send![class, new] };
    let name = NSString::from_str(&format!(
        "vdisplay {}x{}@{}x",
        spec.width,
        spec.height,
        spec.scale()
    ));
    unsafe {
        let _: () = msg_send![&*descriptor, setName: &*name];
        let _: () = msg_send![&*descriptor, setMaxPixelsWide: pixels.0];
        let _: () = msg_send![&*descriptor, setMaxPixelsHigh: pixels.1];
        let _: () = msg_send![&*descriptor, setSizeInMillimeters: CGSize::new(mm.0, mm.1)];
        // sRGB primaries.
        let _: () = msg_send![&*descriptor, setRedPrimary: CGPoint::new(0.6800, 0.3200)];
        let _: () = msg_send![&*descriptor, setGreenPrimary: CGPoint::new(0.2650, 0.6900)];
        let _: () = msg_send![&*descriptor, setBluePrimary: CGPoint::new(0.1500, 0.0600)];
        let _: () = msg_send![&*descriptor, setWhitePoint: CGPoint::new(0.3127, 0.3290)];
        let _: () = msg_send![&*descriptor, setVendorID: VENDOR];
        let _: () = msg_send![&*descriptor, setProductID: slot];
        let _: () = msg_send![&*descriptor, setSerialNum: spec.serial()];
    }
    set_queue(&descriptor);
    Ok(descriptor)
}

/// One mode, listed in points, at the spec's density.
fn settings(spec: Spec) -> anyhow::Result<Retained<AnyObject>> {
    let mode_class = class("CGVirtualDisplayMode")?;
    let allocated: Allocated<AnyObject> = unsafe { msg_send![mode_class, alloc] };
    let mode: Retained<AnyObject> = unsafe {
        msg_send![allocated, initWithWidth: spec.width, height: spec.height, refreshRate: 60.0_f64]
    };
    let modes = NSArray::from_retained_slice(&[mode]);

    let class = class("CGVirtualDisplaySettings")?;
    let settings: Retained<AnyObject> = unsafe { msg_send![class, new] };
    unsafe {
        let _: () = msg_send![&*settings, setHiDPI: u32::from(spec.hidpi)];
        let _: () = msg_send![&*settings, setRotation: 0_u32];
        let _: () = msg_send![&*settings, setModes: &*modes];
    }
    Ok(settings)
}

/// Give the descriptor the main dispatch queue; a reconfigure completes there.
fn set_queue(descriptor: &Retained<AnyObject>) {
    #[link(name = "System", kind = "dylib")]
    unsafe extern "C" {
        /// `dispatch_get_main_queue()` is a macro over this symbol.
        static _dispatch_main_q: std::ffi::c_void;
    }
    let responds: bool = unsafe { msg_send![&**descriptor, respondsToSelector: sel!(setQueue:)] };
    if !responds {
        eprintln!("vdisplay: descriptor has no setQueue:; leaving the queue unset");
        return;
    }
    let queue: *mut AnyObject = (&raw const _dispatch_main_q).cast_mut().cast();
    unsafe {
        let _: () = msg_send![&**descriptor, setQueue: queue];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_under_the_minimum_or_over_the_pixel_limit_is_refused() {
        let ok = Spec { width: 1920, height: 1080, hidpi: true };
        assert!(ok.validate().is_ok());
        assert!(Spec { width: 320, ..ok }.validate().is_err());
        assert!(Spec { width: 4097, ..ok }.validate().is_err());
        assert!(Spec { width: 4097, hidpi: false, ..ok }.validate().is_ok());
    }

    #[test]
    fn an_argument_names_size_and_scale() {
        let spec = Spec::from_arg("1280x800@1").unwrap();
        assert_eq!(spec, Spec { width: 1280, height: 800, hidpi: false });
        assert!(Spec::from_arg("1280x800@3").is_err());
        assert!(Spec::from_arg("1280x800").is_err());
    }

    #[test]
    fn serials_differ_by_size_and_density() {
        let a = Spec { width: 1920, height: 1080, hidpi: true };
        assert_ne!(a.serial(), Spec { hidpi: false, ..a }.serial());
        assert_ne!(a.serial(), Spec { height: 1200, ..a }.serial());
        assert_eq!(a.serial(), 1920 * 100_000 + 1080 * 10 + 2);
    }
}
