//! AppKit overlay: two borderless, transparent, click-through windows — a
//! cursor and a banner — driven by a 60 Hz CFRunLoopTimer. Commands arrive on a
//! shared queue fed by the stdin reader thread; EOF or `quit` exits the process.

#![cfg(target_os = "macos")]
// cocoa 0.26 marks its whole AppKit surface deprecated in favour of the objc2
// crates. This helper deliberately pins cocoa 0.26 (the pointer-based `id` API
// the rest of zode's desktop FFI already uses); the migration is out of scope.
#![allow(deprecated)]

use std::collections::VecDeque;
use std::io::BufRead;
use std::os::raw::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cocoa::appkit::{
    NSApp, NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow,
    NSWindowStyleMask,
};
use cocoa::base::{id, nil, NO, YES};
use cocoa::foundation::{NSPoint, NSRect, NSSize, NSString};
use core_foundation::date::CFAbsoluteTimeGetCurrent;
use core_foundation::runloop::{
    kCFRunLoopCommonModes, CFRunLoop, CFRunLoopTimer, CFRunLoopTimerContext, CFRunLoopTimerRef,
};
use core_graphics::image::CGImage;
use foreign_types::ForeignType;
use objc::{class, msg_send, sel, sel_impl};

use crate::proto::{parse_line, OverlayCmd, Pulse};

const IDLE_HIDE_SECS: f64 = 8.0;
const CHIP_SECS: f64 = 2.0;
const BANNER_W: f64 = 520.0;
const BANNER_H: f64 = 34.0;
const NS_STATUS_WINDOW_LEVEL: i64 = 25;

struct State {
    queue: Arc<Mutex<VecDeque<OverlayCmd>>>,
    eof: Arc<AtomicBool>,
    motion: crate::motion::Motion,
    frames: Vec<CGImage>,
    cur_frame: usize,
    cursor_win: id,
    banner_win: id,
    banner_label: id,
    banner_base: String,
    chip_until: Option<Instant>,
    pending_pulse: Option<Pulse>,
    pulse_frames_left: u32,
    last_cmd: Instant,
    visible: bool,
    screen_h: f64,
    last_window_id: Option<u32>,
}

pub fn run() {
    let queue: Arc<Mutex<VecDeque<OverlayCmd>>> = Arc::default();
    let eof = Arc::new(AtomicBool::new(false));
    {
        let queue = queue.clone();
        let eof = eof.clone();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                let Ok(line) = line else { break };
                if let Some(cmd) = parse_line(&line) {
                    queue.lock().unwrap().push_back(cmd);
                }
            }
            eof.store(true, Ordering::SeqCst);
        });
    }

    unsafe {
        let app = NSApp();
        app.setActivationPolicy_(
            NSApplicationActivationPolicy::NSApplicationActivationPolicyAccessory,
        );
        println!("{{\"ready\":true}}");

        let screen_h = main_screen_height();
        let state = Box::into_raw(Box::new(State {
            queue,
            eof,
            motion: crate::motion::Motion::new(),
            frames: crate::draw::cursor_frames(2.0),
            cur_frame: usize::MAX,
            cursor_win: nil,
            banner_win: nil,
            banner_label: nil,
            banner_base: String::new(),
            chip_until: None,
            pending_pulse: None,
            pulse_frames_left: 0,
            last_cmd: Instant::now(),
            visible: false,
            screen_h,
            last_window_id: None,
        }));

        extern "C" fn tick_cb(_timer: CFRunLoopTimerRef, info: *mut c_void) {
            let state = unsafe { &mut *(info as *mut State) };
            state.tick();
        }
        let mut ctx = CFRunLoopTimerContext {
            version: 0,
            info: state as *mut c_void,
            retain: None,
            release: None,
            copyDescription: None,
        };
        let timer = CFRunLoopTimer::new(
            CFAbsoluteTimeGetCurrent() + 1.0 / 60.0,
            1.0 / 60.0,
            0,
            0,
            tick_cb,
            &mut ctx,
        );
        CFRunLoop::get_current().add_timer(&timer, kCFRunLoopCommonModes);

        app.run();
    }
}

impl State {
    fn tick(&mut self) {
        if self.eof.load(Ordering::SeqCst) {
            std::process::exit(0);
        }
        let cmds: Vec<OverlayCmd> = self.queue.lock().unwrap().drain(..).collect();
        for cmd in cmds {
            self.last_cmd = Instant::now();
            match cmd {
                OverlayCmd::Quit => std::process::exit(0),
                OverlayCmd::Show { banner, esc_hint } => {
                    self.banner_base = format!("{banner} · {esc_hint}");
                    self.show_banner_text(None);
                }
                OverlayCmd::Chip { text } => {
                    self.show_banner_text(Some(&text));
                    self.chip_until = Some(Instant::now());
                }
                OverlayCmd::Hide => self.hide_all(),
                OverlayCmd::Move {
                    x,
                    y,
                    window_id,
                    pulse,
                } => {
                    let ax_y = self.screen_h - y;
                    unsafe { self.ensure_cursor_win() };
                    if !self.visible || (self.motion.is_idle() && self.motion.position().0 < -100.0)
                    {
                        // First appearance: slide in from 140px away, not from
                        // the parking spot offscreen.
                        self.motion
                            .set_position((x - 140.0).max(2.0), (ax_y - 140.0).max(2.0));
                    }
                    self.motion.move_to(x, ax_y);
                    self.pending_pulse = match pulse {
                        Pulse::None => None,
                        p => Some(p),
                    };
                    self.last_window_id = window_id;
                    unsafe { self.order_cursor_win() };
                    self.visible = true;
                }
            }
        }

        // Chip restore.
        if let Some(t0) = self.chip_until {
            if t0.elapsed().as_secs_f64() > CHIP_SECS {
                self.chip_until = None;
                self.show_banner_text(None);
            }
        }

        // Animation.
        let animating = self.motion.tick(1.0 / 60.0);
        if self.visible && self.cursor_win != nil {
            let (x, y) = self.motion.position();
            unsafe {
                let origin =
                    NSPoint::new(x - crate::draw::CANVAS / 2.0, y - crate::draw::CANVAS / 2.0);
                let _: () = msg_send![self.cursor_win, setFrameOrigin: origin];
            }
            let f = crate::draw::frame_for(self.motion.heading());
            if f != self.cur_frame {
                self.cur_frame = f;
                unsafe { self.set_cursor_frame(f) };
            }
        }
        if !animating && self.pending_pulse.take().is_some() {
            self.pulse_frames_left = 8; // ~130ms alpha dip
        }
        if self.pulse_frames_left > 0 {
            self.pulse_frames_left -= 1;
            let alpha: f64 = if self.pulse_frames_left > 4 {
                0.35
            } else {
                1.0
            };
            unsafe {
                let _: () = msg_send![self.cursor_win, setAlphaValue: alpha];
            }
        }

        // Idle hide.
        if self.visible && self.last_cmd.elapsed().as_secs_f64() > IDLE_HIDE_SECS {
            self.hide_all();
        }
    }

    fn hide_all(&mut self) {
        unsafe {
            if self.cursor_win != nil {
                let _: () = msg_send![self.cursor_win, orderOut: nil];
            }
            if self.banner_win != nil {
                let _: () = msg_send![self.banner_win, orderOut: nil];
            }
        }
        self.visible = false;
    }

    fn show_banner_text(&mut self, chip: Option<&str>) {
        unsafe { self.ensure_banner_win() };
        let text = match chip {
            Some(c) => format!("{} · {c}", self.banner_base),
            None => self.banner_base.clone(),
        };
        unsafe {
            let ns = NSString::alloc(nil).init_str(&text);
            let _: () = msg_send![self.banner_label, setStringValue: ns];
            let _: () = msg_send![self.banner_win, orderFrontRegardless];
        }
    }

    unsafe fn ensure_cursor_win(&mut self) {
        if self.cursor_win != nil {
            return;
        }
        let size = crate::draw::CANVAS;
        let win = make_overlay_window(NSRect::new(
            NSPoint::new(-500.0, -500.0),
            NSSize::new(size, size),
        ));
        let view: id = msg_send![win, contentView];
        let _: () = msg_send![view, setWantsLayer: YES];
        self.cursor_win = win;
        self.set_cursor_frame(0);
    }

    unsafe fn set_cursor_frame(&mut self, i: usize) {
        let view: id = msg_send![self.cursor_win, contentView];
        let layer: id = msg_send![view, layer];
        let img = &self.frames[i];
        let _: () = msg_send![layer, setContents: (img.as_ptr() as id)];
    }

    unsafe fn order_cursor_win(&mut self) {
        match self.last_window_id {
            Some(wid) => {
                let _: () = msg_send![self.cursor_win, setLevel: 0i64];
                // NSWindowAbove = 1; window numbers are global CGWindowIDs.
                let _: () = msg_send![self.cursor_win, orderWindow: 1i64 relativeTo: wid as i64];
            }
            None => {
                let _: () = msg_send![self.cursor_win, setLevel: NS_STATUS_WINDOW_LEVEL];
                let _: () = msg_send![self.cursor_win, orderFrontRegardless];
            }
        }
    }

    unsafe fn ensure_banner_win(&mut self) {
        if self.banner_win != nil {
            return;
        }
        let screen_w = main_screen_width();
        let rect = NSRect::new(
            NSPoint::new((screen_w - BANNER_W) / 2.0, self.screen_h - BANNER_H - 8.0),
            NSSize::new(BANNER_W, BANNER_H),
        );
        let win = make_overlay_window(rect);
        let _: () = msg_send![win, setLevel: NS_STATUS_WINDOW_LEVEL];
        let view: id = msg_send![win, contentView];
        let _: () = msg_send![view, setWantsLayer: YES];
        let layer: id = msg_send![view, layer];
        let black: id =
            msg_send![class!(NSColor), colorWithSRGBRed:0.05 green:0.05 blue:0.06 alpha:0.82];
        let cg: id = msg_send![black, CGColor];
        let _: () = msg_send![layer, setBackgroundColor: cg];
        let _: () = msg_send![layer, setCornerRadius: (BANNER_H / 2.0)];

        let inner = NSRect::new(
            NSPoint::new(12.0, 6.0),
            NSSize::new(BANNER_W - 24.0, BANNER_H - 12.0),
        );
        let label: id = msg_send![class!(NSTextField), alloc];
        let label: id = msg_send![label, initWithFrame: inner];
        let _: () = msg_send![label, setBezeled: NO];
        let _: () = msg_send![label, setDrawsBackground: NO];
        let _: () = msg_send![label, setEditable: NO];
        let _: () = msg_send![label, setSelectable: NO];
        let white: id = msg_send![class!(NSColor), whiteColor];
        let _: () = msg_send![label, setTextColor: white];
        let font: id = msg_send![class!(NSFont), systemFontOfSize: 13.0];
        let _: () = msg_send![label, setFont: font];
        let _: () = msg_send![label, setAlignment: 1i64]; // NSTextAlignmentCenter
        let _: () = msg_send![view, addSubview: label];

        self.banner_win = win;
        self.banner_label = label;
    }
}

unsafe fn make_overlay_window(rect: NSRect) -> id {
    let win: id = msg_send![class!(NSWindow), alloc];
    let win: id = win.initWithContentRect_styleMask_backing_defer_(
        rect,
        NSWindowStyleMask::NSBorderlessWindowMask,
        NSBackingStoreType::NSBackingStoreBuffered,
        NO,
    );
    let _: () = msg_send![win, setOpaque: NO];
    let clear: id = msg_send![class!(NSColor), clearColor];
    let _: () = msg_send![win, setBackgroundColor: clear];
    let _: () = msg_send![win, setHasShadow: NO];
    let _: () = msg_send![win, setIgnoresMouseEvents: YES];
    let _: () = msg_send![win, setReleasedWhenClosed: NO];
    // CanJoinAllSpaces (1<<0) | Stationary (1<<4) | FullScreenAuxiliary (1<<8).
    let behavior: u64 = (1u64 << 0) | (1u64 << 4) | (1u64 << 8);
    let _: () = msg_send![win, setCollectionBehavior: behavior];
    win
}

unsafe fn main_screen_height() -> f64 {
    let screen: id = msg_send![class!(NSScreen), mainScreen];
    let frame: NSRect = msg_send![screen, frame];
    frame.size.height
}

unsafe fn main_screen_width() -> f64 {
    let screen: id = msg_send![class!(NSScreen), mainScreen];
    let frame: NSRect = msg_send![screen, frame];
    frame.size.width
}
