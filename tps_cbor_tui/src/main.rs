/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! `cbor-tui` — terminal workbench for CDDL/CBOR/EDN/COSE workflows.

mod app;
mod artifact;
mod cose_ops;
mod error;
mod event;
mod highlight;
mod keystore;
mod ui;
mod validate;

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::widgets::{Block, Borders, Paragraph};

use app::{App, Mode};
use artifact::{Artifact, ArtifactView};
use error::AppError;
use event::AppEvent;

/// Number of lines a Page Up/Down scrolls.
const PAGE_SIZE: u16 = 10;

#[derive(Parser)]
#[command(name = "cbor-tui", version, about = "Terminal workbench for CDDL/CBOR/EDN/COSE")]
struct Cli {
    /// CBOR file to open at startup
    file: Option<PathBuf>,

    /// Directory holding the flat-file COSE key store
    #[arg(long, default_value = "cose_keys")]
    keystore_dir: PathBuf,
}

fn main() -> Result<(), AppError> {
    let cli = Cli::parse();

    let artifact = match cli.file {
        Some(path) => {
            let bytes = std::fs::read(&path)?;
            Artifact::from_file(path, bytes)
        }
        None => Artifact::new(Vec::new()),
    };

    let mut terminal = init_terminal()?;
    install_panic_hook();

    let app = App::new(artifact, &cli.keystore_dir)?;
    let result = run(&mut terminal, app);

    restore_terminal(&mut terminal)?;
    result
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>, AppError> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<(), AppError> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

/// Ensure the terminal is restored even if the app panics, so the shell isn't left broken.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, mut app: App) -> Result<(), AppError> {
    while !app.should_quit {
        terminal.draw(|frame| draw(frame, &mut app))?;

        match event::next_event(Duration::from_millis(200))? {
            AppEvent::Key(key) => {
                // Force a full repaint after scrolling: some Windows terminals
                // (mintty/ConPTY) have been observed to drift from ratatui's
                // diff-based redraw when a pane's content shifts, leaving
                // stale glyphs on screen. A full clear is cheap (only paid on
                // a scroll keypress, not every frame) and guarantees the
                // terminal resyncs with the actual buffer contents.
                let is_scroll = matches!(
                    key.code,
                    crossterm::event::KeyCode::Up
                        | crossterm::event::KeyCode::Down
                        | crossterm::event::KeyCode::PageUp
                        | crossterm::event::KeyCode::PageDown
                );
                handle_key(&mut app, key)?;
                if is_scroll {
                    terminal.clear()?;
                }
            }
            AppEvent::Resize | AppEvent::Tick => {}
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) -> Result<(), AppError> {
    use crossterm::event::{KeyCode, KeyModifiers};

    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
        app.clear_status();
        return Ok(());
    }

    if app.confirm_overwrite.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => app.confirm_yes(),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => app.confirm_no(),
            _ => {}
        }
        return Ok(());
    }

    if app.input_prompt.is_some() {
        match key.code {
            KeyCode::Esc => app.cancel_prompt(),
            KeyCode::Enter => app.submit_prompt(),
            KeyCode::Char(c) => app.input_prompt.as_mut().unwrap().editor.insert_char(c),
            KeyCode::Backspace => app.input_prompt.as_mut().unwrap().editor.backspace(),
            KeyCode::Delete => app.input_prompt.as_mut().unwrap().editor.delete_forward(),
            KeyCode::Left => app.input_prompt.as_mut().unwrap().editor.move_left(),
            KeyCode::Right => app.input_prompt.as_mut().unwrap().editor.move_right(),
            _ => {}
        }
        return Ok(());
    }

    let loadable = matches!(app.mode, Mode::CborView | Mode::EdnEdit | Mode::HexEdit | Mode::CddlEdit);
    if loadable && key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('o') => {
                app.start_load_prompt();
                return Ok(());
            }
            KeyCode::Char('s') => {
                app.request_save();
                return Ok(());
            }
            _ => {}
        }
    }

    if app.mode == Mode::CddlEdit && app.cddl_diag_focus {
        match key.code {
            KeyCode::Esc | KeyCode::Tab => app.unfocus_cddl_diagnostics(),
            KeyCode::Up => app.cddl_diag_move_selection(-1),
            KeyCode::Down => app.cddl_diag_move_selection(1),
            KeyCode::Enter => app.jump_to_selected_cddl_diagnostic(),
            _ => {}
        }
        return Ok(());
    }

    let editing_text = matches!(app.mode, Mode::EdnEdit | Mode::HexEdit | Mode::CddlEdit);

    if editing_text {
        let editor_has_error = matches!(app.mode, Mode::EdnEdit | Mode::HexEdit) && app.status.is_some();
        match key.code {
            KeyCode::Esc if editor_has_error => app.cancel_edit(),
            KeyCode::Esc => match app.mode {
                Mode::EdnEdit => app.commit_edn(),
                Mode::HexEdit => app.commit_hex(),
                _ => app.mode = Mode::CborView,
            },
            KeyCode::Char('q') if editor_has_error => app.should_quit = true,
            KeyCode::Tab if app.mode == Mode::CddlEdit => app.focus_cddl_diagnostics(),
            KeyCode::F(5) if app.mode == Mode::EdnEdit => app.start_check_prompt(),
            KeyCode::Char(c) => {
                editor_for(app).insert_char(c);
                if app.mode == Mode::CddlEdit {
                    app.revalidate_cddl();
                }
            }
            KeyCode::Backspace => {
                editor_for(app).backspace();
                if app.mode == Mode::CddlEdit {
                    app.revalidate_cddl();
                }
            }
            KeyCode::Delete => {
                editor_for(app).delete_forward();
                if app.mode == Mode::CddlEdit {
                    app.revalidate_cddl();
                }
            }
            KeyCode::Left => editor_for(app).move_left(),
            KeyCode::Right => editor_for(app).move_right(),
            KeyCode::Up => app.scroll_up(1),
            KeyCode::Down => app.scroll_down(1),
            KeyCode::PageUp => app.scroll_up(PAGE_SIZE),
            KeyCode::PageDown => app.scroll_down(PAGE_SIZE),
            KeyCode::Enter => {
                editor_for(app).insert_char('\n');
                if app.mode == Mode::CddlEdit {
                    app.revalidate_cddl();
                }
            }
            _ => {}
        }
        return Ok(());
    }

    if app.mode == Mode::KeyStore {
        match key.code {
            KeyCode::Esc => app.mode = Mode::CborView,
            KeyCode::Up => app.keystore_move_selection(-1),
            KeyCode::Down => app.keystore_move_selection(1),
            KeyCode::Enter => app.set_active_key(),
            KeyCode::Char('g') => app.start_generate_prompt(),
            KeyCode::Char('i') => app.start_import_prompt(),
            KeyCode::Char('x') => app.delete_selected_key(),
            KeyCode::Char('s') => app.mode = Mode::CoseSign,
            KeyCode::Char('e') => app.mode = Mode::CoseEncrypt,
            KeyCode::Char('c') => app.copy_hex_to_clipboard(),
            _ => {}
        }
        return Ok(());
    }

    if app.mode == Mode::CoseSign {
        match key.code {
            KeyCode::Esc => app.mode = Mode::CborView,
            KeyCode::Char('s') => app.sign_artifact(),
            KeyCode::Char('v') => app.verify_artifact(),
            _ => {}
        }
        return Ok(());
    }

    if app.mode == Mode::CoseEncrypt {
        match key.code {
            KeyCode::Esc => app.mode = Mode::CborView,
            KeyCode::Char('e') => app.encrypt_artifact(),
            KeyCode::Char('d') => app.decrypt_artifact(),
            _ => {}
        }
        return Ok(());
    }

    if app.mode == Mode::CborCheck {
        match key.code {
            KeyCode::Esc => app.exit_check(),
            KeyCode::Up => app.check_move_selection(-1),
            KeyCode::Down => app.check_move_selection(1),
            _ => {}
        }
        return Ok(());
    }

    if event::is_quit(&key) {
        app.should_quit = true;
        return Ok(());
    }

    match key.code {
        KeyCode::Tab => app.toggle_view(),
        KeyCode::F(2) => app.mode = Mode::CddlEdit,
        KeyCode::F(3) => app.mode = Mode::KeyStore,
        KeyCode::F(4) => app.start_hex_edit(),
        KeyCode::F(5) if app.mode == Mode::CborView => app.start_check_prompt(),
        KeyCode::Char('c') => app.copy_hex_to_clipboard(),
        KeyCode::Up => app.scroll_up(1),
        KeyCode::Down => app.scroll_down(1),
        KeyCode::PageUp => app.scroll_up(PAGE_SIZE),
        KeyCode::PageDown => app.scroll_down(PAGE_SIZE),
        _ => {}
    }
    Ok(())
}

fn editor_for(app: &mut App) -> &mut crate::ui::text_editor::TextEditor {
    match app.mode {
        Mode::CddlEdit => &mut app.cddl_editor,
        Mode::HexEdit => &mut app.hex_editor,
        _ => &mut app.edn_editor,
    }
}

fn draw(frame: &mut ratatui::Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    let mut cursor: Option<ratatui::layout::Position> = None;

    if app.mode == Mode::CddlEdit {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[0]);

        let source_block = Block::default().title("CDDL").borders(Borders::ALL);
        let source_inner = source_block.inner(panes[0]);
        app.cddl_pane_width = source_inner.width;
        let source_lines = ui::wrap::wrap_lines(
            highlight::cddl::highlight_cddl(app.cddl_editor.text()),
            source_inner.width as usize,
        );
        frame.render_widget(
            Paragraph::new(source_lines).block(source_block).scroll((app.cddl_scroll, 0)),
            panes[0],
        );

        let diag_lines: Vec<ratatui::text::Line> = app
            .cddl_diagnostics
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let (line, col) = tps_cddl::ast::offset_to_line_col(app.cddl_editor.text(), d.span.start);
                let text = format!("{}:{}: {}", line, col, d.message);
                if app.cddl_diag_focus && i == app.cddl_diag_selected {
                    ratatui::text::Line::from(text).style(ratatui::style::Style::default().add_modifier(
                        ratatui::style::Modifier::REVERSED,
                    ))
                } else {
                    ratatui::text::Line::from(text)
                }
            })
            .collect();
        let diag_title = if app.cddl_diag_focus {
            "Diagnostics (↑↓ select, Enter jump, Esc back)"
        } else {
            "Diagnostics (Tab to focus)"
        };
        let diag_block = Block::default().title(diag_title).borders(Borders::ALL);
        frame.render_widget(Paragraph::new(diag_lines).block(diag_block), panes[1]);

        if !app.cddl_diag_focus {
            let (row, col) = app.cddl_editor.cursor_row_col();
            let visual = ui::wrap::visual_position(app.cddl_editor.text(), row, col, source_inner.width as usize);
            cursor = cursor_position_in(source_inner, visual, app.cddl_scroll);
        }
    } else if app.mode == Mode::KeyStore {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[0]);

        let lines = ui::keystore_view::render_key_list(
            app.keystore.list(),
            app.keystore_selected,
            app.active_key_alias.as_deref(),
        );
        let block = Block::default().title("Key store").borders(Borders::ALL);
        frame.render_widget(Paragraph::new(lines).block(block), panes[0]);

        let hex_lines = app
            .keystore
            .list()
            .get(app.keystore_selected)
            .and_then(|meta| app.keystore.load(&meta.alias).ok())
            .map(|bytes| ui::cbor_view::render_hex_dump(&bytes, 16))
            .unwrap_or_else(|| vec![ratatui::text::Line::from("(no key selected)")]);
        let hex_block = Block::default().title("Key bytes").borders(Borders::ALL);
        frame.render_widget(Paragraph::new(hex_lines).block(hex_block), panes[1]);
    } else if app.mode == Mode::CoseSign {
        let lines = ui::cose_sign::render_sign_screen(
            app.active_key_alias.as_deref(),
            app.artifact.bytes.len(),
            app.status.as_deref(),
        );
        let block = Block::default().title("COSE sign/verify").borders(Borders::ALL);
        frame.render_widget(Paragraph::new(lines).block(block), chunks[0]);
    } else if app.mode == Mode::CoseEncrypt {
        let lines = ui::cose_encrypt::render_encrypt_screen(
            app.active_key_alias.as_deref(),
            app.artifact.bytes.len(),
            app.status.as_deref(),
        );
        let block = Block::default().title("COSE encrypt/decrypt").borders(Borders::ALL);
        frame.render_widget(Paragraph::new(lines).block(block), chunks[0]);
    } else if app.mode == Mode::CborCheck {
        let title = match &app.check_root_rule {
            Some(rule) => format!("Check against CDDL rule '{rule}' (Esc: back)"),
            None => "Check against CDDL first rule (Esc: back)".to_owned(),
        };
        let lines: Vec<ratatui::text::Line> = if let Some(err) = &app.check_error {
            vec![ratatui::text::Line::from(format!("error: {err}"))]
        } else if app.check_mismatches.is_empty() {
            vec![ratatui::text::Line::from("conforms to schema — no mismatches")]
        } else {
            app.check_mismatches
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let text = format!("{}: expected {}, got {}", m.path, m.expected, m.got);
                    if i == app.check_selected {
                        ratatui::text::Line::from(text).style(
                            ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::REVERSED),
                        )
                    } else {
                        ratatui::text::Line::from(text)
                    }
                })
                .collect()
        };
        let block = Block::default().title(title).borders(Borders::ALL);
        frame.render_widget(Paragraph::new(lines).block(block), chunks[0]);
    } else {
        match app.artifact.view {
            ArtifactView::Hex if app.mode == Mode::HexEdit => {
                let lines: Vec<ratatui::text::Line> =
                    app.hex_editor.text().lines().map(ratatui::text::Line::from).collect();
                let block = Block::default().title("CBOR (hex, editing)").borders(Borders::ALL);
                let inner = block.inner(chunks[0]);
                frame.render_widget(
                    Paragraph::new(lines).block(block).scroll((app.hex_edit_scroll, 0)),
                    chunks[0],
                );
                cursor = cursor_position_in(inner, app.hex_editor.cursor_row_col(), app.hex_edit_scroll);
            }
            ArtifactView::Hex => {
                let lines = ui::cbor_view::render_hex_dump(&app.artifact.bytes, 16);
                let block = Block::default().title("CBOR (hex)").borders(Borders::ALL);
                frame.render_widget(
                    Paragraph::new(lines).block(block).scroll((app.cbor_scroll, 0)),
                    chunks[0],
                );
            }
            ArtifactView::Edn => {
                let block = Block::default().title("EDN").borders(Borders::ALL);
                let inner = block.inner(chunks[0]);
                app.edn_pane_width = inner.width;
                let lines = ui::wrap::wrap_lines(highlight::edn::highlight_edn(app.edn_editor.text()), inner.width as usize);
                frame.render_widget(
                    Paragraph::new(lines).block(block).scroll((app.edn_scroll, 0)),
                    chunks[0],
                );
                if app.mode == Mode::EdnEdit {
                    let (row, col) = app.edn_editor.cursor_row_col();
                    let visual = ui::wrap::visual_position(app.edn_editor.text(), row, col, inner.width as usize);
                    cursor = cursor_position_in(inner, visual, app.edn_scroll);
                }
            }
        }
    }

    if let Some(prompt) = &app.input_prompt {
        let label = match prompt.kind {
            app::InputKind::GenerateAlias => "alias for new key: ",
            app::InputKind::ImportPath => "path to COSE key file: ",
            app::InputKind::LoadCborPath => "path to CBOR/EDN file to load: ",
            app::InputKind::LoadCddlPath => "path to CDDL file to load: ",
            app::InputKind::SavePath => "save to path: ",
            app::InputKind::CheckRootRule => "CDDL rule to check against (blank = first rule): ",
        };
        let area = centered_rect(chunks[0], 60, 3);
        let block = Block::default().title("Input").borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(
            Paragraph::new(format!("{label}{}", prompt.editor.text())).block(block),
            area,
        );
        let (_, col) = prompt.editor.cursor_row_col();
        cursor = Some(ratatui::layout::Position::new(
            (inner.x + label.len() as u16 + col).min(inner.right().saturating_sub(1)),
            inner.y,
        ));
    }

    if let Some(pending) = &app.confirm_overwrite {
        let area = centered_rect(chunks[0], 60, 3);
        let block = Block::default().title("Confirm overwrite").borders(Borders::ALL);
        frame.render_widget(
            Paragraph::new(format!("{} exists — overwrite? (y/n)", pending.path.display())).block(block),
            area,
        );
    }

    let mode_hint = match app.mode {
        Mode::CddlEdit => {
            "Esc: apply  ↑↓/PgUp/PgDn: scroll  Tab: focus diagnostics  Ctrl+O: load  Ctrl+S: save  Ctrl+L: clear msg  (editing CDDL)"
        }
        Mode::EdnEdit => "Esc: apply  ↑↓/PgUp/PgDn: scroll  Ctrl+O: load  Ctrl+S: save  F5: check vs CDDL  Ctrl+L: clear msg  q: quit (on error)  (editing EDN)",
        Mode::HexEdit => "Esc: apply  ↑↓/PgUp/PgDn: scroll  Ctrl+O: load  Ctrl+S: save  Ctrl+L: clear msg  q: quit (on error)  (editing hex)",
        Mode::KeyStore => "Enter: select  g: generate  i: import  x: delete  c: copy hex  s: sign  e: encrypt  Esc: back  Ctrl+L: clear msg",
        Mode::CoseSign => "s: sign  v: verify  Esc: back",
        Mode::CoseEncrypt => "e: encrypt  d: decrypt  Esc: back",
        Mode::CborCheck => "↑↓: select  Esc: back",
        Mode::CborView => {
            "Tab: toggle hex/EDN  ↑↓/PgUp/PgDn: scroll  F2: CDDL editor  F3: key store  F4: edit hex  F5: check vs CDDL  c: copy hex  Ctrl+O: load  Ctrl+S: save  Ctrl+L: clear msg  q/Esc: quit"
        }
    };
    let status = ui::status_bar::render_status_line(mode_hint, &app.artifact.provenance, app.status.as_deref());
    frame.render_widget(Paragraph::new(status), chunks[1]);

    if let Some(pos) = cursor {
        frame.set_cursor_position(pos);
    }
}

/// Screen position of a `(row, col)` text-editor cursor inside `inner`
/// (a block's content area), given the pane's current vertical scroll
/// offset — `None` if the cursor's row has scrolled out of view.
fn cursor_position_in(
    inner: ratatui::layout::Rect,
    (row, col): (u16, u16),
    scroll: u16,
) -> Option<ratatui::layout::Position> {
    if row < scroll {
        return None;
    }
    let screen_row = row - scroll;
    if screen_row >= inner.height {
        return None;
    }
    Some(ratatui::layout::Position::new(
        inner.x + col.min(inner.width.saturating_sub(1)),
        inner.y + screen_row,
    ))
}

fn centered_rect(area: ratatui::layout::Rect, width_pct: u16, height: u16) -> ratatui::layout::Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(height),
            Constraint::Fill(1),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_pct) / 2),
            Constraint::Percentage(width_pct),
            Constraint::Percentage((100 - width_pct) / 2),
        ])
        .split(vertical[1])[1]
}
