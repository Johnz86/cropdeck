use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, ImageFormat, MapState, Window,
};
use x11rb::rust_connection::RustConnection;

use crate::screenshot::{ScreenSize, WindowRect};

const ACTIVATION_SETTLE: Duration = Duration::from_millis(400);
const NAME_LENGTH_LIMIT: u32 = 1_024;
const ACTIVATION_SOURCE_PAGER: u32 = 2;

pub struct WindowPixels {
    pub depth: u8,
    pub bytes: Vec<u8>,
}

pub struct Desktop {
    connection: RustConnection,
    root: Window,
    screen: ScreenSize,
}

impl Desktop {
    pub fn connect() -> Result<Self> {
        let (connection, screen_number) =
            x11rb::connect(None).context("no X11 display is reachable")?;
        let screen = &connection.setup().roots[screen_number];
        let size = ScreenSize {
            width: u32::from(screen.width_in_pixels),
            height: u32::from(screen.height_in_pixels),
        };
        let root = screen.root;

        Ok(Self {
            connection,
            root,
            screen: size,
        })
    }

    #[must_use]
    pub const fn screen_size(&self) -> ScreenSize {
        self.screen
    }

    pub fn find_window(&self, wanted: &str) -> Result<Option<Window>> {
        let mut named = Vec::new();
        self.collect_named(self.root, &mut named)?;
        let exact = named
            .iter()
            .find(|(_window, name)| name == wanted)
            .map(|(window, _name)| *window);
        if exact.is_some() {
            return Ok(exact);
        }

        Ok(named
            .into_iter()
            .find(|(_window, name)| name.contains(wanted))
            .map(|(window, _name)| window))
    }

    pub fn rect(&self, window: Window) -> Result<WindowRect> {
        let geometry = self
            .connection
            .get_geometry(window)?
            .reply()
            .context("window geometry is unavailable")?;
        let position = self
            .connection
            .translate_coordinates(window, self.root, 0, 0)?
            .reply()
            .context("window position is unavailable")?;

        Ok(WindowRect {
            x: i32::from(position.dst_x),
            y: i32::from(position.dst_y),
            width: u32::from(geometry.width),
            height: u32::from(geometry.height),
        })
    }

    pub fn occluder(&self, window: Window, rect: WindowRect) -> Result<Option<String>> {
        let toplevel = self.toplevel_ancestor(window)?;
        let stacking = self.connection.query_tree(self.root)?.reply()?.children;
        let Some(position) = stacking.iter().position(|child| *child == toplevel) else {
            return Ok(None);
        };
        for candidate in &stacking[position + 1..] {
            if !self.is_viewable(*candidate)? {
                continue;
            }
            let candidate_rect = self.rect(*candidate)?;
            if candidate_rect.width <= 1 || candidate_rect.height <= 1 {
                continue;
            }
            if candidate_rect.overlaps(rect) {
                let name = self.window_name(*candidate)?;
                return Ok(Some(match name {
                    Some(name) => format!("'{name}'"),
                    None => format!("window {candidate:#x}"),
                }));
            }
        }

        Ok(None)
    }

    pub fn activate(&self, window: Window) -> Result<()> {
        let toplevel = self.toplevel_ancestor(window)?;
        let active = self
            .connection
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")?
            .reply()?
            .atom;
        let message = ClientMessageEvent::new(
            32,
            toplevel,
            active,
            [ACTIVATION_SOURCE_PAGER, x11rb::CURRENT_TIME, 0, 0, 0],
        );
        self.connection.send_event(
            false,
            self.root,
            EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
            message,
        )?;
        self.connection.flush()?;
        sleep(ACTIVATION_SETTLE);

        Ok(())
    }

    pub fn pixels(&self, window: Window, rect: WindowRect) -> Result<WindowPixels> {
        let width = u16::try_from(rect.width).context("window is too wide to capture")?;
        let height = u16::try_from(rect.height).context("window is too tall to capture")?;
        let image = self
            .connection
            .get_image(ImageFormat::Z_PIXMAP, window, 0, 0, width, height, !0)?
            .reply()
            .context("window pixels are unavailable")?;

        Ok(WindowPixels {
            depth: image.depth,
            bytes: image.data,
        })
    }

    fn collect_named(&self, window: Window, found: &mut Vec<(Window, String)>) -> Result<()> {
        for child in self.connection.query_tree(window)?.reply()?.children {
            if let Some(name) = self.window_name(child)? {
                found.push((child, name));
            }
            self.collect_named(child, found)?;
        }

        Ok(())
    }

    fn window_name(&self, window: Window) -> Result<Option<String>> {
        let reply = self
            .connection
            .get_property(
                false,
                window,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                0,
                NAME_LENGTH_LIMIT,
            )?
            .reply();
        let Ok(reply) = reply else {
            return Ok(None);
        };
        if reply.value.is_empty() {
            return Ok(None);
        }

        Ok(Some(String::from_utf8_lossy(&reply.value).into_owned()))
    }

    fn is_viewable(&self, window: Window) -> Result<bool> {
        let Ok(attributes) = self.connection.get_window_attributes(window)?.reply() else {
            return Ok(false);
        };

        Ok(attributes.map_state == MapState::VIEWABLE)
    }

    fn toplevel_ancestor(&self, window: Window) -> Result<Window> {
        let mut current = window;
        loop {
            let tree = self.connection.query_tree(current)?.reply()?;
            if tree.parent == self.root || tree.parent == x11rb::NONE {
                return Ok(current);
            }
            current = tree.parent;
        }
    }
}
