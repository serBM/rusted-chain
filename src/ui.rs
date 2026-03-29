use std::sync::{Arc, Mutex};
use crate::effects::{EffectSlot, Distortion, Bitcrusher, Delay, Chorus, Compressor, Reverb, Effect, AVAILABLE_EFFECTS};
use crate::preset::{Preset, effects_to_preset, preset_to_effects};

#[derive(PartialEq)]
pub enum Panel {
    Left,
    Right,
}

pub struct AppState {
    pub focused_panel: Panel,
    pub list_state: ratatui::widgets::ListState,
    pub selected_effect: usize,
    pub selected_parameter: usize,
    pub grabbing: bool,
    pub show_popup: bool,
    pub popup_selected: usize,
    pub input_mode: bool,
    pub input_buffer: String,
    pub show_load_popup: bool,
    pub load_popup_files: Vec<String>,
    pub load_popup_selected: usize,
}

pub fn run_ui(effects: Arc<Mutex<Vec<EffectSlot>>>, volume: Arc<Mutex<f32>>) {
    let mut state = AppState {
        focused_panel: Panel::Left,
        list_state: ratatui::widgets::ListState::default(),
        selected_effect: 0,
        selected_parameter: 0,
        grabbing: false,
        show_popup: false,
        popup_selected: 0,
        input_mode: false,
        input_buffer: String::new(),
        show_load_popup: false,
        load_popup_files: Vec::new(),
        load_popup_selected: 0,
    };
    state.list_state.select(Some(0));

    crossterm::terminal::enable_raw_mode().unwrap();
    crossterm::execute!(std::io::stdout(), crossterm::terminal::Clear(crossterm::terminal::ClearType::All)).unwrap();
    let mut terminal = ratatui::Terminal::new(
        ratatui::backend::CrosstermBackend::new(std::io::stdout())
    ).unwrap();

    loop {
        terminal.draw(|frame| {
            let effects_lock = effects.lock().unwrap();
            let items: Vec<ratatui::widgets::ListItem> = effects_lock
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let item = ratatui::widgets::ListItem::new(e.effect.name());
                    if state.grabbing && i == state.selected_effect {
                        item.style(ratatui::style::Style::default().fg(ratatui::style::Color::Yellow))
                    } else {
                        item
                    }
                })
                .collect();
            let highlight_style = ratatui::style::Style::default().fg(ratatui::style::Color::Yellow);
            let normal_style = ratatui::style::Style::default();
            let param_items: Vec<ratatui::widgets::ListItem> = if effects_lock.is_empty() {
                vec![]
            } else {
                let selected_slot = &effects_lock[state.selected_effect];
                let wet_item = ratatui::widgets::ListItem::new(format!("wet: {}", selected_slot.wet))
                    .style(if state.focused_panel == Panel::Right && state.selected_parameter == 0 { highlight_style } else { normal_style });
                let mut items = vec![wet_item];
                let names = selected_slot.effect.param_names();
                let values = selected_slot.effect.param_values();
                items.extend(
                    names.iter().zip(values.iter()).enumerate()
                        .map(|(i, (name, value))| {
                            ratatui::widgets::ListItem::new(format!("{}: {}", name, value))
                                .style(if state.focused_panel == Panel::Right && state.selected_parameter == i + 1 { highlight_style } else { normal_style })
                        })
                );
                items
            };
            let vertical = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    ratatui::layout::Constraint::Length(3),
                    ratatui::layout::Constraint::Min(0),
                    ratatui::layout::Constraint::Length(3),
                ])
                .split(frame.area());
            let key_style = ratatui::style::Style::default().fg(ratatui::style::Color::Yellow).add_modifier(ratatui::style::Modifier::BOLD);
            let desc_style = ratatui::style::Style::default();
            let keybind_pairs: &[(&str, &str)] = if state.input_mode {
                &[("Enter", "confirm"), ("Esc", "cancel")]
            } else if state.show_load_popup {
                &[("↑↓", "navigate"), ("Enter", "load"), ("Esc", "cancel")]
            } else if state.show_popup {
                &[("↑↓", "navigate"), ("Enter", "confirm"), ("Esc", "cancel")]
            } else if state.focused_panel == Panel::Left {
                &[("↑↓", "navigate"), ("Enter", "grab/release"), ("Del", "delete"), ("A", "add"), ("S", "save"), ("L", "load"), ("Tab", "switch panel"), ("Q", "quit")]
            } else {
                &[("↑↓", "navigate"), ("←→", "change value"), ("S", "save"), ("L", "load"), ("Tab", "switch panel"), ("Q", "quit")]
            };
            let keybind_spans: Vec<ratatui::text::Span> = keybind_pairs.iter().flat_map(|(key, desc)| {
                vec![
                    ratatui::text::Span::styled(format!(" {} ", key), key_style),
                    ratatui::text::Span::styled(format!("{} ", desc), desc_style),
                ]
            }).collect();
            frame.render_widget(
                ratatui::widgets::Paragraph::new(ratatui::text::Line::from(keybind_spans))
                    .block(ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::ALL)),
                vertical[0],
            );
            let areas = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Horizontal)
                .constraints([
                    ratatui::layout::Constraint::Percentage(30),
                    ratatui::layout::Constraint::Percentage(70),
                ])
                .split(vertical[1]);
            let vol = *volume.lock().unwrap();
            frame.render_widget(
                ratatui::widgets::Paragraph::new(format!("Volume: {:.2}", vol))
                    .block(ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::ALL)),
                vertical[2],
            );
            let left_border_style = if state.focused_panel == Panel::Left { highlight_style } else { normal_style };
            let right_border_style = if state.focused_panel == Panel::Right { highlight_style } else { normal_style };
            frame.render_widget(
                ratatui::widgets::List::new(param_items)
                    .block(ratatui::widgets::Block::default().title("Parameters").borders(ratatui::widgets::Borders::ALL).border_style(right_border_style)),
                areas[1],
            );
            frame.render_stateful_widget(
                ratatui::widgets::List::new(items)
                    .block(ratatui::widgets::Block::default().title("Effects").borders(ratatui::widgets::Borders::ALL).border_style(left_border_style))
                    .highlight_symbol(">> "),
                areas[0],
                &mut state.list_state,
            );
            if state.input_mode {
                let popup_area = ratatui::layout::Rect {
                    x: frame.area().width / 4,
                    y: frame.area().height / 2 - 2,
                    width: frame.area().width / 2,
                    height: 3,
                };
                frame.render_widget(ratatui::widgets::Clear, popup_area);
                frame.render_widget(
                    ratatui::widgets::Paragraph::new(state.input_buffer.as_str())
                        .block(ratatui::widgets::Block::default().title("Save preset as").borders(ratatui::widgets::Borders::ALL)),
                    popup_area,
                );
            }
            if state.show_load_popup {
                let popup_items: Vec<ratatui::widgets::ListItem> = state.load_popup_files.iter().enumerate()
                    .map(|(i, name)| {
                        ratatui::widgets::ListItem::new(name.as_str())
                            .style(if i == state.load_popup_selected { highlight_style } else { normal_style })
                    })
                    .collect();
                let height = (state.load_popup_files.len() as u16 + 2).max(3);
                let popup_area = ratatui::layout::Rect {
                    x: frame.area().width / 4,
                    y: frame.area().height / 4,
                    width: frame.area().width / 2,
                    height,
                };
                frame.render_widget(ratatui::widgets::Clear, popup_area);
                frame.render_widget(
                    ratatui::widgets::List::new(popup_items)
                        .block(ratatui::widgets::Block::default().title("Load preset").borders(ratatui::widgets::Borders::ALL)),
                    popup_area,
                );
            }
            if state.show_popup {
                let popup_items: Vec<ratatui::widgets::ListItem> = AVAILABLE_EFFECTS.iter().enumerate()
                    .map(|(i, name)| {
                        ratatui::widgets::ListItem::new(*name)
                            .style(if i == state.popup_selected { highlight_style } else { normal_style })
                    })
                    .collect();
                let popup_area = ratatui::layout::Rect {
                    x: frame.area().width / 4,
                    y: frame.area().height / 4,
                    width: frame.area().width / 2,
                    height: AVAILABLE_EFFECTS.len() as u16 + 2,
                };
                frame.render_widget(ratatui::widgets::Clear, popup_area);
                frame.render_widget(
                    ratatui::widgets::List::new(popup_items)
                        .block(ratatui::widgets::Block::default().title("Add Effect").borders(ratatui::widgets::Borders::ALL)),
                    popup_area,
                );
            }
        }).unwrap();

        if crossterm::event::poll(std::time::Duration::from_millis(16)).unwrap() {
            if let crossterm::event::Event::Key(key) = crossterm::event::read().unwrap() {
                if key.code == crossterm::event::KeyCode::Char('q') || (key.code == crossterm::event::KeyCode::Char('c') && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)) {
                    crossterm::terminal::disable_raw_mode().unwrap();
                    break;
                }
                if state.input_mode {
                    match key.code {
                        crossterm::event::KeyCode::Esc => {
                            state.input_mode = false;
                            state.input_buffer.clear();
                        }
                        crossterm::event::KeyCode::Enter => {
                            let name = state.input_buffer.trim().to_string();
                            if !name.is_empty() {
                                let preset = effects_to_preset(name.clone(), &effects.lock().unwrap());
                                let dir = crate::preset::preset_dir();
                                std::fs::create_dir_all(&dir).unwrap();
                                let path = dir.join(format!("{}.json", name));
                                std::fs::write(path, serde_json::to_string_pretty(&preset).unwrap()).unwrap();
                            }
                            state.input_mode = false;
                            state.input_buffer.clear();
                        }
                        crossterm::event::KeyCode::Backspace => { state.input_buffer.pop(); }
                        crossterm::event::KeyCode::Char(c) => { state.input_buffer.push(c); }
                        _ => {}
                    }
                } else if !state.show_popup {
                    if key.code == crossterm::event::KeyCode::Enter && state.focused_panel == Panel::Left {
                        state.grabbing = !state.grabbing;
                    }
                    if key.code == crossterm::event::KeyCode::Up && !effects.lock().unwrap().is_empty() && !state.show_load_popup {
                        if state.focused_panel == Panel::Left {
                            if state.grabbing {
                                let mut effects_lock = effects.lock().unwrap();
                                if state.selected_effect > 0 {
                                    effects_lock.swap(state.selected_effect, state.selected_effect - 1);
                                    state.selected_effect -= 1;
                                    state.list_state.select(Some(state.selected_effect));
                                }
                            } else {
                                if state.selected_effect > 0 { state.selected_effect -= 1; } else { state.selected_effect = effects.lock().unwrap().len() - 1; }
                                state.list_state.select(Some(state.selected_effect));
                            }
                        } else if state.focused_panel == Panel::Right {
                            let param_count = 1 + effects.lock().unwrap()[state.selected_effect].effect.param_names().len();
                            if state.selected_parameter > 0 { state.selected_parameter -= 1; } else { state.selected_parameter = param_count - 1; }
                        }
                    }
                    if key.code == crossterm::event::KeyCode::Down && !effects.lock().unwrap().is_empty() && !state.show_load_popup {
                        if state.focused_panel == Panel::Left {
                            if state.grabbing {
                                let mut effects_lock = effects.lock().unwrap();
                                if state.selected_effect < effects_lock.len() - 1 {
                                    effects_lock.swap(state.selected_effect, state.selected_effect + 1);
                                    state.selected_effect += 1;
                                    state.list_state.select(Some(state.selected_effect));
                                }
                            } else {
                                if state.selected_effect < effects.lock().unwrap().len() - 1 { state.selected_effect += 1; } else { state.selected_effect = 0; }
                                state.list_state.select(Some(state.selected_effect));
                            }
                        } else if state.focused_panel == Panel::Right {
                            let param_count = 1 + effects.lock().unwrap()[state.selected_effect].effect.param_names().len();
                            if state.selected_parameter < param_count - 1 { state.selected_parameter += 1; } else { state.selected_parameter = 0; }
                        }
                    }
                    if key.code == crossterm::event::KeyCode::Left && state.focused_panel == Panel::Right && !effects.lock().unwrap().is_empty() {
                        let mut effects_lock = effects.lock().unwrap();
                        let slot = &mut effects_lock[state.selected_effect];
                        if state.selected_parameter == 0 {
                            slot.wet = (slot.wet - 0.05).clamp(0.0, 1.0);
                        } else {
                            slot.effect.adjust_param(state.selected_parameter - 1, -1.0);
                        }
                    }
                    if key.code == crossterm::event::KeyCode::Right && state.focused_panel == Panel::Right && !effects.lock().unwrap().is_empty() {
                        let mut effects_lock = effects.lock().unwrap();
                        let slot = &mut effects_lock[state.selected_effect];
                        if state.selected_parameter == 0 {
                            slot.wet = (slot.wet + 0.05).clamp(0.0, 1.0);
                        } else {
                            slot.effect.adjust_param(state.selected_parameter - 1, 1.0);
                        }
                    }
                    if key.code == crossterm::event::KeyCode::PageUp {
                        let mut vol = volume.lock().unwrap();
                        *vol = (*vol + 0.05).min(2.0);
                    }
                    if key.code == crossterm::event::KeyCode::PageDown {
                        let mut vol = volume.lock().unwrap();
                        *vol = (*vol - 0.05).max(0.0);
                    }
                    if key.code == crossterm::event::KeyCode::Delete && state.focused_panel == Panel::Left {
                        let mut effects_lock = effects.lock().unwrap();
                        if !effects_lock.is_empty() {
                            effects_lock.remove(state.selected_effect);
                            if state.selected_effect >= effects_lock.len() && state.selected_effect > 0 {
                                state.selected_effect -= 1;
                            }
                            state.list_state.select(Some(state.selected_effect));
                        }
                    }
                } // end !show_popup
                if key.code == crossterm::event::KeyCode::Char('s') && !state.show_popup && !state.input_mode && !state.show_load_popup {
                    state.input_mode = true;
                    state.input_buffer.clear();
                }
                if key.code == crossterm::event::KeyCode::Char('l') && !state.show_popup && !state.input_mode && !state.show_load_popup {
                    let dir = crate::preset::preset_dir();
                    let files: Vec<String> = std::fs::read_dir(&dir).ok()
                        .into_iter().flatten()
                        .filter_map(|e| e.ok())
                        .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
                        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().to_string()))
                        .collect();
                    state.load_popup_files = files;
                    state.load_popup_selected = 0;
                    state.show_load_popup = true;
                }
                if state.show_load_popup {
                    if key.code == crossterm::event::KeyCode::Esc {
                        state.show_load_popup = false;
                    }
                    if key.code == crossterm::event::KeyCode::Up && !state.load_popup_files.is_empty() {
                        if state.load_popup_selected > 0 { state.load_popup_selected -= 1; } else { state.load_popup_selected = state.load_popup_files.len() - 1; }
                    }
                    if key.code == crossterm::event::KeyCode::Down && !state.load_popup_files.is_empty() {
                        if state.load_popup_selected < state.load_popup_files.len() - 1 { state.load_popup_selected += 1; } else { state.load_popup_selected = 0; }
                    }
                    if key.code == crossterm::event::KeyCode::Enter && !state.load_popup_files.is_empty() {
                        let name = &state.load_popup_files[state.load_popup_selected];
                        let path = crate::preset::preset_dir().join(format!("{}.json", name));
                        if let Ok(contents) = std::fs::read_to_string(path) {
                            if let Ok(preset) = serde_json::from_str::<Preset>(&contents) {
                                *effects.lock().unwrap() = preset_to_effects(preset);
                                state.selected_effect = 0;
                                state.list_state.select(Some(0));
                            }
                        }
                        state.show_load_popup = false;
                    }
                }
                if key.code == crossterm::event::KeyCode::Char('a') && state.focused_panel == Panel::Left && !state.show_popup && !state.input_mode {
                    state.show_popup = true;
                    state.popup_selected = 0;
                }
                if state.show_popup {
                    if key.code == crossterm::event::KeyCode::Esc {
                        state.show_popup = false;
                    }
                    if key.code == crossterm::event::KeyCode::Up {
                        if state.popup_selected > 0 { state.popup_selected -= 1; } else { state.popup_selected = AVAILABLE_EFFECTS.len() - 1; }
                    }
                    if key.code == crossterm::event::KeyCode::Down {
                        if state.popup_selected < AVAILABLE_EFFECTS.len() - 1 { state.popup_selected += 1; } else { state.popup_selected = 0; }
                    }
                    if key.code == crossterm::event::KeyCode::Enter {
                        let new_effect: Box<dyn Effect + Send> = match state.popup_selected {
                            0 => Box::new(Distortion { drive: 2.0, hard: false }),
                            1 => Box::new(Bitcrusher { bit_depth: 8 }),
                            2 => Box::new(Delay { past_left_signal: Vec::new(), past_right_signal: Vec::new(), delay_ms: 300.0, decay: 0.4, ping_pong: false }),
                            3 => Box::new(Chorus { past_left_signal: Vec::new(), past_right_signal: Vec::new(), delay_ms: 30.0, depth_ms: 2.0, lfo_frequency: 1.0, lfo_phase: 0.0 }),
                            4 => Box::new(Compressor { threshold: 0.7, ratio: 4.0, attack_ms: 10.0, release_ms: 200.0, current_gain: 1.0 }),
                            _ => Box::new(Reverb::new(1.0, 0.5)),
                        };
                        effects.lock().unwrap().push(EffectSlot { effect: new_effect, enabled: true, wet: 1.0 });
                        state.show_popup = false;
                    }
                }
                if key.code == crossterm::event::KeyCode::Tab && !state.show_popup {
                    state.focused_panel = if state.focused_panel == Panel::Left { Panel::Right } else { Panel::Left };
                }
            }
        }
    }
}
