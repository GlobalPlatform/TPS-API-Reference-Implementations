/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! crossterm event polling -> application-level events.

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use std::time::Duration;

pub enum AppEvent {
    Key(KeyEvent),
    Resize,
    Tick,
}

/// Poll for the next terminal event, returning `Tick` if none arrived within `timeout`.
pub fn next_event(timeout: Duration) -> std::io::Result<AppEvent> {
    if event::poll(timeout)? {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => Ok(AppEvent::Key(key)),
            Event::Key(_) => Ok(AppEvent::Tick),
            Event::Resize(_, _) => Ok(AppEvent::Resize),
            _ => Ok(AppEvent::Tick),
        }
    } else {
        Ok(AppEvent::Tick)
    }
}

pub fn is_quit(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
}
