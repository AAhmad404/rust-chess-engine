use crate::position::Position;
use crate::search::{GoParams, SearchInfo, Searcher};
use chess::ChessMove;
use std::io::{self, BufRead, Write};
use std::str::{FromStr, SplitWhitespace};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

const ENGINE_NAME: &str = "Rust Chess Engine";
const ENGINE_AUTHOR: &str = "user";

pub fn run() {
    let command_epoch = Arc::new(AtomicU64::new(0));
    let ponder_signal = Arc::new(AtomicBool::new(false));
    let mut position = Position::default();
    let searcher = Arc::new(Mutex::new(Searcher::new(
        32,
        Arc::clone(&command_epoch),
        Arc::clone(&ponder_signal),
    )));
    let stdin = io::stdin();
    let mut debug_enabled = false;
    let mut game_start_time_ms = None;

    // Search runs on a worker so the protocol loop can answer commands immediately.
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            break;
        };
        let Some(command) = command_from_line(line.trim()) else {
            continue;
        };
        let keyword = command.split_whitespace().next().unwrap_or("");
        let epoch = if matches!(keyword, "go" | "stop" | "quit" | "position" | "ucinewgame") {
            command_epoch.fetch_add(1, Ordering::Relaxed) + 1
        } else {
            command_epoch.load(Ordering::Relaxed)
        };
        match keyword {
            "uci" => {
                send(&format!("id name {ENGINE_NAME}"));
                send(&format!("id author {ENGINE_AUTHOR}"));
                send("option name Clear Hash type button");
                send("option name Ponder type check default false");
                send("uciok");
            }
            "isready" => send("readyok"),
            "ucinewgame" => {
                position = Position::default();
                game_start_time_ms = None;
                lock_searcher(&searcher).clear();
            }
            "position" => match Position::from_uci(&command) {
                Ok(next) => {
                    position = next;
                    if debug_enabled {
                        send(&format!(
                            "info string position hash {}",
                            position.board.get_hash()
                        ));
                    }
                }
                Err(error) => send(&format!("info string {error}")),
            },
            "setoption" => match parse_setoption(&command) {
                Ok(SetOption::Hash(_)) => send("info string Hash size is not configurable"),
                Ok(SetOption::ClearHash) => lock_searcher(&searcher).clear(),
                Ok(SetOption::Ponder | SetOption::Unknown) => {}
                Err(error) => send(&format!("info string {error}")),
            },
            "go" => {
                let mut params = match parse_go(&command) {
                    Ok(params) => params,
                    Err(error) => {
                        send(&format!("info string {error}"));
                        continue;
                    }
                };
                if game_start_time_ms.is_none() {
                    game_start_time_ms = params
                        .white_time_ms
                        .into_iter()
                        .chain(params.black_time_ms)
                        .max();
                }
                params.game_start_time_ms = game_start_time_ms;
                if debug_enabled {
                    send(&format!(
                        "info string search start epoch {epoch} hash {}",
                        position.board.get_hash()
                    ));
                }
                ponder_signal.store(params.ponder, Ordering::Relaxed);
                let search_position = position.clone();
                let searcher = Arc::clone(&searcher);
                thread::spawn(move || {
                    let result = lock_searcher(&searcher).search(
                        &search_position,
                        &params,
                        epoch,
                        report_search_info,
                    );
                    match (result.best_move, result.ponder_move) {
                        (Some(best_move), Some(ponder_move)) => {
                            send(&format!("bestmove {best_move} ponder {ponder_move}"));
                        }
                        (Some(best_move), None) => send(&format!("bestmove {best_move}")),
                        (None, _) => send("bestmove 0000"),
                    }
                });
            }
            "ponderhit" => {
                ponder_signal.store(false, Ordering::Relaxed);
            }
            "debug" => match command.split_whitespace().nth(1) {
                Some("on") => {
                    debug_enabled = true;
                    send("info string debug enabled");
                }
                Some("off") => {
                    debug_enabled = false;
                    send("info string debug disabled");
                }
                _ => send("info string expected 'debug on' or 'debug off'"),
            },
            "stop" => {}
            "d" => send(&format!("info string {}", position.board)),
            "eval" => send(&format!(
                "info string evaluation {} cp",
                crate::evaluation::evaluate(&position.board)
            )),
            "quit" => break,
            "" | "register" => {}
            _ => {}
        }
    }

    command_epoch.fetch_add(1, Ordering::Relaxed);
}

fn command_from_line(line: &str) -> Option<String> {
    if line.starts_with('#') {
        return None;
    }

    const COMMANDS: &[&str] = &[
        "uci",
        "debug",
        "isready",
        "setoption",
        "register",
        "ucinewgame",
        "position",
        "go",
        "stop",
        "ponderhit",
        "quit",
        "d",
        "eval",
    ];
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let index = tokens.iter().position(|token| COMMANDS.contains(token))?;
    Some(tokens[index..].join(" "))
}

fn lock_searcher(searcher: &Mutex<Searcher>) -> std::sync::MutexGuard<'_, Searcher> {
    searcher
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn send(message: &str) {
    println!("{message}");
    let _ = io::stdout().flush();
}

fn report_search_info(info: SearchInfo) {
    let elapsed_ms = info.elapsed.as_millis();
    let nodes = u128::from(info.nodes);
    let scaled_nodes = nodes.saturating_mul(1_000);
    let nps = scaled_nodes.checked_div(elapsed_ms).unwrap_or(scaled_nodes);
    let score = format_score(info.score);
    let pv = info
        .pv
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    send(&format!(
        "info depth {} score {} nodes {} time {} nps {} pv {}",
        info.depth, score, info.nodes, elapsed_ms, nps, pv
    ));
}

fn format_score(score: i32) -> String {
    format!("cp {score}")
}

#[derive(Debug, PartialEq, Eq)]
enum SetOption {
    Hash(usize),
    ClearHash,
    Ponder,
    Unknown,
}

fn parse_setoption(command: &str) -> Result<SetOption, String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    if tokens.first() != Some(&"setoption") || tokens.get(1) != Some(&"name") {
        return Err("expected 'setoption name <name> [value <value>]'".into());
    }
    let value_index = tokens.iter().position(|token| *token == "value");
    let name_end = value_index.unwrap_or(tokens.len());
    let name = tokens[2..name_end].join(" ");
    let value = value_index.map(|index| tokens[index + 1..].join(" "));

    match name.to_ascii_lowercase().as_str() {
        "hash" => {
            let value = value
                .filter(|text| !text.is_empty())
                .ok_or_else(|| "Hash option requires a value".to_string())?
                .parse::<usize>()
                .map_err(|_| "invalid Hash value".to_string())?;
            Ok(SetOption::Hash(value.clamp(1, 1024)))
        }
        "clear hash" => Ok(SetOption::ClearHash),
        "ponder" => {
            let value = value.ok_or_else(|| "Ponder option requires a value".to_string())?;
            value
                .to_ascii_lowercase()
                .parse::<bool>()
                .map_err(|_| "invalid Ponder value".to_string())?;
            Ok(SetOption::Ponder)
        }
        _ => Ok(SetOption::Unknown),
    }
}

pub fn parse_go(command: &str) -> Result<GoParams, String> {
    let mut tokens = command.split_whitespace();
    if tokens.next() != Some("go") {
        return Err("expected a go command".into());
    }

    let mut params = GoParams::default();
    while let Some(parameter) = tokens.next() {
        match parameter {
            "searchmoves" => {
                for text in tokens.by_ref() {
                    params.search_moves.push(
                        ChessMove::from_str(text)
                            .map_err(|_| format!("invalid searchmoves move: {text}"))?,
                    );
                }
                if params.search_moves.is_empty() {
                    return Err("searchmoves requires at least one move".into());
                }
                break;
            }
            "wtime" => params.white_time_ms = Some(parse_u64(&mut tokens, parameter)?),
            "btime" => params.black_time_ms = Some(parse_u64(&mut tokens, parameter)?),
            "winc" => params.white_increment_ms = parse_u64(&mut tokens, parameter)?,
            "binc" => params.black_increment_ms = parse_u64(&mut tokens, parameter)?,
            "movestogo" => {
                params.moves_to_go = Some(parse_u64(&mut tokens, parameter)?.max(1));
            }
            "movetime" => {
                params.move_time_ms = Some(parse_u64(&mut tokens, parameter)?.max(1));
            }
            "nodes" => params.nodes = Some(parse_u64(&mut tokens, parameter)?.max(1)),
            "depth" => {
                params.depth = Some(parse_u64(&mut tokens, parameter)?.clamp(1, 127) as u8);
            }
            "mate" => {
                params.mate = Some(parse_u64(&mut tokens, parameter)?.clamp(1, 63) as u8);
            }
            "infinite" => params.infinite = true,
            "ponder" => params.ponder = true,
            _ => {}
        }
    }
    Ok(params)
}

fn parse_u64(tokens: &mut SplitWhitespace<'_>, parameter: &str) -> Result<u64, String> {
    tokens
        .next()
        .ok_or_else(|| format!("{parameter} requires a value"))?
        .parse::<u64>()
        .map_err(|_| format!("invalid {parameter} value"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clock_go_command() {
        let params = parse_go("go wtime 10000 btime 9000 winc 100 binc 50").unwrap();
        assert_eq!(params.white_time_ms, Some(10_000));
        assert_eq!(params.black_increment_ms, 50);
    }

    #[test]
    fn parses_flag_go_parameters_without_consuming_the_next_parameter() {
        let params = parse_go("go ponder wtime 10000 infinite").unwrap();
        assert!(params.infinite);
        assert!(params.ponder);
        assert_eq!(params.white_time_ms, Some(10_000));
    }

    #[test]
    fn parses_searchmoves_as_the_final_go_parameter() {
        let params = parse_go("go depth 5 searchmoves e2e4 d2d4").unwrap();
        assert_eq!(params.depth, Some(5));
        assert_eq!(params.search_moves.len(), 2);
        assert_eq!(params.search_moves[0].to_string(), "e2e4");
    }

    #[test]
    fn parses_supported_options() {
        assert_eq!(
            parse_setoption("setoption name Hash value 64").unwrap(),
            SetOption::Hash(64)
        );
        assert_eq!(
            parse_setoption("setoption name Clear Hash").unwrap(),
            SetOption::ClearHash
        );
        assert_eq!(
            parse_setoption("setoption name Ponder value TRUE").unwrap(),
            SetOption::Ponder
        );
    }

    #[test]
    fn ignores_unknown_tokens() {
        assert_eq!(
            command_from_line("unknown debug on").as_deref(),
            Some("debug on")
        );
        let params = parse_go("go extension depth 4").unwrap();
        assert_eq!(params.depth, Some(4));
    }

    #[test]
    fn rejects_non_go_commands() {
        assert!(parse_go("depth 5").is_err());
    }

    #[test]
    fn formats_internal_scores_as_centipawns() {
        assert_eq!(format_score(i32::MIN), "cp -2147483648");
        assert_eq!(format_score(i32::MAX), "cp 2147483647");
        assert_eq!(format_score(178), "cp 178");
    }
}
