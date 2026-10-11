use std::env;

use xcb::{
    Connection as XcbConnection, Xid,
    randr::{GetMonitors, MonitorInfoBuf, Output},
    x::{Atom, InternAtom, ScreenBuf},
};

use crate::{XCapError, error::XCapResult};

pub fn get_xcb_connection_and_index() -> XCapResult<(XcbConnection, i32)> {
    let display = env::var("DISPLAY").unwrap_or_else(|_| "DISPLAY:1".to_string());
    let (conn, idx) = XcbConnection::connect(Some(display.as_str()))
        .or_else(|_| XcbConnection::connect(None))
        .map_err(|e| XCapError::new(e.to_string()))?;
    Ok((conn, idx))
}

pub fn get_current_screen_buf() -> XCapResult<ScreenBuf> {
    let (conn, index) = get_xcb_connection_and_index()?;

    let setup = conn.get_setup();

    let screen = setup
        .roots()
        .nth(index as usize)
        .ok_or_else(|| XCapError::new("Not found screen"))?;

    Ok(screen.to_owned())
}

pub fn get_monitor_info_buf(output: Output) -> XCapResult<MonitorInfoBuf> {
    let (conn, _) = get_xcb_connection_and_index()?;

    let screen_buf = get_current_screen_buf()?;

    let get_monitors_cookie = conn.send_request(&GetMonitors {
        window: screen_buf.root(),
        get_active: true,
    });

    let get_monitors_reply = conn.wait_for_reply(get_monitors_cookie)?;

    let monitor_info_iterator = get_monitors_reply.monitors();

    for monitor_info in monitor_info_iterator {
        for &item in monitor_info.outputs() {
            if item == output {
                return Ok(monitor_info.to_owned());
            }
        }
    }
    Err(XCapError::new("Not found monitor"))
}

pub fn get_atom(name: &str) -> XCapResult<Atom> {
    let (conn, _) = get_xcb_connection_and_index()?;
    let atom_cookie = conn.send_request(&InternAtom {
        only_if_exists: true,
        name: name.as_bytes(),
    });
    let atom_reply = conn.wait_for_reply(atom_cookie)?;
    let atom = atom_reply.atom();

    if atom.is_none() {
        return Err(XCapError::new(format!("{name} not supported")));
    }

    Ok(atom)
}
