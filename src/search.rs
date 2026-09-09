use crate::compat::{generation_key, insufficient_material, move_key, sort_root};
use crate::evaluation::evaluate;
use crate::position::{Position, ep_file, is_capture, next_halfmove_clock};
use chess::{Board, BoardStatus, ChessMove, Color, MoveGen};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
#[derive(Clone, Debug, Default)]
pub struct GoParams {
    pub search_moves: Vec<ChessMove>,
    pub white_time_ms: Option<u64>,
    pub black_time_ms: Option<u64>,
    pub white_increment_ms: u64,
    pub black_increment_ms: u64,
    pub moves_to_go: Option<u64>,
    pub game_start_time_ms: Option<u64>,
    pub move_time_ms: Option<u64>,
    pub depth: Option<u8>,
    pub mate: Option<u8>,
    pub nodes: Option<u64>,
    pub infinite: bool,
    pub ponder: bool,
}

#[derive(Clone, Debug)]
pub struct SearchInfo {
    pub depth: u8,
    pub score: i32,
    pub nodes: u64,
    pub elapsed: Duration,
    pub pv: Vec<ChessMove>,
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub best_move: Option<ChessMove>,
    pub ponder_move: Option<ChessMove>,
}

#[derive(Default)]
struct IntegerHasher(u64);

impl Hasher for IntegerHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0 = bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
        });
    }

    fn write_u32(&mut self, value: u32) {
        self.0 = mix_integer(u64::from(value));
    }

    fn write_u128(&mut self, value: u128) {
        self.0 = mix_integer(value as u64 ^ (value >> 64) as u64);
    }
}

fn mix_integer(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

type IntegerMap<K, V> = HashMap<K, V, BuildHasherDefault<IntegerHasher>>;

const MAX_DEPTH: u8 = 40;
const MATE_SCORE: i32 = 1_000_000;
const FIRST_KILLER_BONUS: i32 = 9_000;
const SECOND_KILLER_BONUS: i32 = 8_000;
const PIECE_VALUES: [i32; 6] = [100, 300, 325, 500, 900, 100_000];

pub struct Searcher {
    evaluations: IntegerMap<u128, i32>,
    move_table: IntegerMap<u32, i32>,
    killer_moves: [[Option<u32>; 2]; MAX_DEPTH as usize + 1],
    command_epoch: Arc<AtomicU64>,
    ponder_signal: Arc<std::sync::atomic::AtomicBool>,
    search_epoch: u64,
    started: Instant,
    time_budget: Option<Duration>,
    deadline: Option<Instant>,
    pondering: bool,
    node_limit: Option<u64>,
    nodes: u64,
    pub positions_evaluated: u64,
    pub prunes: u64,
    position_hashes: Vec<(u64, u8)>,
}
impl Searcher {
    pub fn new(
        _hash_mb: usize,
        command_epoch: Arc<AtomicU64>,
        ponder_signal: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            evaluations: IntegerMap::default(),
            move_table: IntegerMap::default(),
            killer_moves: [[None; 2]; MAX_DEPTH as usize + 1],
            command_epoch,
            ponder_signal,
            search_epoch: 0,
            started: Instant::now(),
            time_budget: None,
            deadline: None,
            pondering: false,
            node_limit: None,
            nodes: 0,
            positions_evaluated: 0,
            prunes: 0,
            position_hashes: Vec::new(),
        }
    }
    pub fn search<F>(
        &mut self,
        position: &Position,
        params: &GoParams,
        search_epoch: u64,
        mut report: F,
    ) -> SearchResult
    where
        F: FnMut(SearchInfo),
    {
        self.started = Instant::now();
        self.search_epoch = search_epoch;
        self.nodes = 0;
        self.positions_evaluated = 0;
        self.prunes = 0;
        self.killer_moves = [[None; 2]; MAX_DEPTH as usize + 1];
        self.node_limit = params.nodes;
        self.position_hashes.clone_from(&position.hashes);
        self.time_budget = time_budget(position.board.side_to_move(), params);
        self.pondering = params.ponder && self.ponder_signal.load(Ordering::Relaxed);
        self.deadline = if self.pondering {
            None
        } else {
            self.time_budget.map(|budget| self.started + budget)
        };
        let mut moves = self.order_moves(&position.board, 0);
        if !params.search_moves.is_empty() {
            moves.retain(|m| params.search_moves.contains(m));
        }
        if moves.is_empty() {
            return SearchResult {
                best_move: None,
                ponder_move: None,
            };
        }
        let mut best_move = moves[0];
        let max_depth = max_search_depth(params);
        let mut mate_found = false;
        for depth in 1..=max_depth {
            let mut scored = Vec::with_capacity(moves.len());
            let white = position.board.side_to_move() == Color::White;
            let mut alpha = i32::MIN;
            let mut beta = i32::MAX;
            let mut iteration_best = None;
            for &m in &moves {
                let child = position.board.make_move_new(m);
                let clock = next_halfmove_clock(&position.board, m, position.halfmove_clock);
                let child_ep = ep_file(&position.board, m);
                self.position_hashes.push((child.get_hash(), child_ep));
                let score = self.alpha_beta(
                    &child,
                    (depth - 1, depth),
                    alpha,
                    beta,
                    (clock, child_ep),
                    1,
                );
                self.position_hashes.pop();
                let Some(score) = score else {
                    break;
                };
                let winning_mate = if white {
                    score >= MATE_SCORE - i32::from(MAX_DEPTH)
                } else {
                    score <= -MATE_SCORE + i32::from(MAX_DEPTH)
                };
                if winning_mate {
                    best_move = m;
                    report(SearchInfo {
                        depth,
                        score: score_for_side_to_move(score, position.board.side_to_move()),
                        nodes: self.nodes,
                        elapsed: self.started.elapsed(),
                        pv: vec![m],
                    });
                    mate_found = true;
                    break;
                }
                scored.push((score, m));
                if iteration_best.is_none_or(
                    |(best, _)| {
                        if white { score > best } else { score < best }
                    },
                ) {
                    iteration_best = Some((score, m));
                }
                if white {
                    alpha = alpha.max(score);
                } else {
                    beta = beta.min(score);
                }
                if self.deadline_reached() {
                    break;
                }
            }
            if mate_found {
                break;
            }
            if scored.len() != moves.len() || self.interrupted() || self.deadline_reached() {
                break;
            }
            let (score, chosen) = iteration_best.unwrap();
            order_root_scores(&mut scored, position.board.side_to_move());
            let chosen_index = scored.iter().position(|&(_, m)| m == chosen).unwrap();
            scored.swap(0, chosen_index);
            best_move = chosen;
            moves = scored.into_iter().map(|(_, m)| m).collect();
            report(SearchInfo {
                depth,
                score: score_for_side_to_move(score, position.board.side_to_move()),
                nodes: self.nodes,
                elapsed: self.started.elapsed(),
                pv: vec![chosen],
            });
            if self.deadline.is_some_and(|d| Instant::now() >= d) {
                break;
            }
        }
        while params.infinite || self.pondering {
            self.refresh_ponder_state();
            if self.command_changed() || (!params.infinite && !self.pondering) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        SearchResult {
            best_move: Some(best_move),
            ponder_move: None,
        }
    }
    pub fn clear(&mut self) {
        self.evaluations.clear();
        self.move_table.clear();
        self.killer_moves = [[None; 2]; MAX_DEPTH as usize + 1];
    }
    pub fn evaluate_cached(&mut self, board: &Board) -> i32 {
        self.evaluate_with_ep(
            board,
            board
                .en_passant()
                .map_or(0, |s| s.get_file().to_index() as u8 + 1),
        )
    }

    fn evaluate_with_ep(&mut self, board: &Board, ep: u8) -> i32 {
        let hash = (u128::from(board.get_hash()) << 8) | u128::from(ep);
        if let Some(&score) = self.evaluations.get(&hash) {
            return score;
        }
        self.positions_evaluated += 1;
        let score = evaluate(board);
        self.evaluations.insert(hash, score);
        score
    }
    pub fn clear_evaluations(&mut self) {
        self.evaluations.clear();
    }
    fn alpha_beta(
        &mut self,
        board: &Board,
        depths: (u8, u8),
        mut alpha: i32,
        mut beta: i32,
        history: (u16, u8),
        search_level: usize,
    ) -> Option<i32> {
        let (depth, quiescence_depth) = depths;
        if depth == 0 {
            return self.quiescence_search(
                board,
                quiescence_depth,
                alpha,
                beta,
                history,
                search_level,
            );
        }

        let (clock, ep) = history;
        self.nodes += 1;
        if self.interrupted() {
            return None;
        }
        if self.deadline_reached() {
            return Some(0);
        }
        let white = board.side_to_move() == Color::White;
        let moves = self.order_moves(board, search_level);
        if moves.is_empty() {
            return Some(if board.checkers().popcnt() == 0 {
                0
            } else if white {
                -MATE_SCORE + search_level as i32
            } else {
                MATE_SCORE - search_level as i32
            });
        }
        if clock >= 100
            || insufficient_material(board)
            || self.is_repetition((board.get_hash(), ep), clock)
        {
            return Some(0);
        }
        let mut best = if white { i32::MIN } else { i32::MAX };
        for m in moves {
            let child = board.make_move_new(m);
            let clock = next_halfmove_clock(board, m, clock);
            let child_ep = ep_file(board, m);
            self.position_hashes.push((child.get_hash(), child_ep));
            let value = self.alpha_beta(
                &child,
                (depth - 1, quiescence_depth),
                alpha,
                beta,
                (clock, child_ep),
                search_level + 1,
            );
            self.position_hashes.pop();
            let value = value?;
            let key = move_key(board, m);
            self.move_table.insert(key, value);
            if white {
                best = best.max(value);
                alpha = alpha.max(value);
            } else {
                best = best.min(value);
                beta = beta.min(value);
            }
            if beta <= alpha {
                self.record_killer_move(board, m, search_level);
                self.prunes += 1;
                break;
            }
        }
        Some(best)
    }

    fn quiescence_search(
        &mut self,
        board: &Board,
        depth: u8,
        mut alpha: i32,
        mut beta: i32,
        history: (u16, u8),
        search_level: usize,
    ) -> Option<i32> {
        self.nodes += 1;
        if self.interrupted() {
            return None;
        }
        if self.deadline_reached() {
            return Some(0);
        }

        let (clock, ep) = history;
        let white = board.side_to_move() == Color::White;
        let status = board.status();
        if status == BoardStatus::Checkmate {
            return Some(if white {
                -MATE_SCORE + search_level as i32
            } else {
                MATE_SCORE - search_level as i32
            });
        }
        if clock >= 100
            || insufficient_material(board)
            || status == BoardStatus::Stalemate
            || self.is_repetition((board.get_hash(), ep), clock)
        {
            return Some(0);
        }

        let in_check = board.checkers().popcnt() != 0;
        let stand_pat = self.evaluate_with_ep(board, ep);
        if depth == 0 {
            return Some(stand_pat);
        }

        if !in_check {
            if white {
                if stand_pat >= beta {
                    return Some(stand_pat);
                }
                alpha = alpha.max(stand_pat);
            } else {
                if stand_pat <= alpha {
                    return Some(stand_pat);
                }
                beta = beta.min(stand_pat);
            }
        }

        let tactical_moves: Vec<_> = MoveGen::new_legal(board)
            .filter(|&m| in_check || self.is_tactical_move(board, m))
            .collect();
        if tactical_moves.is_empty() {
            return Some(stand_pat);
        }
        let moves = self.order_move_list(board, tactical_moves, search_level);
        let mut best = if in_check {
            if white { i32::MIN } else { i32::MAX }
        } else {
            stand_pat
        };

        for m in moves {
            let child = board.make_move_new(m);
            let child_clock = next_halfmove_clock(board, m, clock);
            let child_ep = ep_file(board, m);
            self.position_hashes.push((child.get_hash(), child_ep));
            let value = self.quiescence_search(
                &child,
                depth - 1,
                alpha,
                beta,
                (child_clock, child_ep),
                search_level + 1,
            );
            self.position_hashes.pop();
            let value = value?;

            if white {
                best = best.max(value);
                alpha = alpha.max(value);
            } else {
                best = best.min(value);
                beta = beta.min(value);
            }
            if beta <= alpha {
                self.prunes += 1;
                break;
            }
        }
        Some(best)
    }

    fn is_tactical_move(&self, board: &Board, m: ChessMove) -> bool {
        is_capture(board, m)
            || m.get_promotion().is_some()
            || board.make_move_new(m).checkers().popcnt() != 0
    }

    fn order_moves(&self, board: &Board, search_level: usize) -> Vec<ChessMove> {
        self.order_move_list(board, MoveGen::new_legal(board).collect(), search_level)
    }

    fn order_move_list(
        &self,
        board: &Board,
        moves: Vec<ChessMove>,
        search_level: usize,
    ) -> Vec<ChessMove> {
        let mut scored: Vec<_> = moves
            .into_iter()
            .map(|m| {
                (
                    (
                        std::cmp::Reverse(self.priority(board, m, search_level)),
                        generation_key(board, m),
                    ),
                    m,
                )
            })
            .collect();
        scored.sort_unstable_by_key(|entry| entry.0);
        scored.into_iter().map(|(_, m)| m).collect()
    }
    fn priority(&self, board: &Board, m: ChessMove, search_level: usize) -> i32 {
        let key = move_key(board, m);
        let mut priority = 0;
        if let Some(&score) = self.move_table.get(&key) {
            let relative = if board.side_to_move() == Color::White {
                i64::from(score)
            } else {
                -i64::from(score)
            };
            priority += relative.clamp(-100, 100) as i32;
        }
        let child = board.make_move_new(m);
        if child.checkers().popcnt() != 0 {
            priority += 30_000;
            if child.status() == BoardStatus::Checkmate {
                priority += 1_000_000;
            }
        }
        let moved_piece = board.piece_on(m.get_source()).unwrap();
        if is_capture(board, m) {
            let captured_piece = board.piece_on(m.get_dest()).unwrap_or(chess::Piece::Pawn);
            priority += 10_000 + 100 * (captured_piece.to_index() as i32 + 1)
                - (moved_piece.to_index() as i32 + 1);
        }
        if let Some(promotion) = m.get_promotion() {
            priority += 20_000 + PIECE_VALUES[promotion.to_index()];
        }
        if !is_capture(board, m)
            && m.get_promotion().is_none()
            && search_level <= MAX_DEPTH as usize
        {
            if self.killer_moves[search_level][0] == Some(key) {
                priority += FIRST_KILLER_BONUS;
            } else if self.killer_moves[search_level][1] == Some(key) {
                priority += SECOND_KILLER_BONUS;
            }
        }
        if (key >> 12) & 15 == 2 {
            priority += 200;
        }
        priority
    }
    fn record_killer_move(&mut self, board: &Board, m: ChessMove, search_level: usize) {
        if is_capture(board, m) || m.get_promotion().is_some() || search_level > MAX_DEPTH as usize
        {
            return;
        }
        let key = move_key(board, m);
        if self.killer_moves[search_level][0] != Some(key) {
            self.killer_moves[search_level][1] = self.killer_moves[search_level][0];
            self.killer_moves[search_level][0] = Some(key);
        }
    }
    fn is_repetition(&self, hash: (u64, u8), clock: u16) -> bool {
        let end = self.position_hashes.len().saturating_sub(1);
        let start = end.saturating_sub(clock as usize);
        self.position_hashes[start..end].contains(&hash)
    }
    fn command_changed(&self) -> bool {
        self.command_epoch.load(Ordering::Relaxed) != self.search_epoch
    }
    fn refresh_ponder_state(&mut self) {
        if self.pondering && !self.ponder_signal.load(Ordering::Relaxed) {
            self.pondering = false;
            self.deadline = self.time_budget.map(|budget| Instant::now() + budget);
        }
    }
    fn interrupted(&mut self) -> bool {
        self.refresh_ponder_state();
        self.node_limit.is_some_and(|limit| self.nodes >= limit) || self.command_changed()
    }
    fn deadline_reached(&self) -> bool {
        self.deadline.is_some_and(|d| Instant::now() >= d)
    }
}

fn score_for_side_to_move(score: i32, side: Color) -> i32 {
    if side == Color::White { score } else { -score }
}

fn order_root_scores(items: &mut [(i32, ChessMove)], side: Color) {
    sort_root(items);
    if side == Color::White {
        items.reverse();
    }
}

fn max_search_depth(params: &GoParams) -> u8 {
    params
        .depth
        .or_else(|| params.mate.map(|depth| depth.saturating_mul(2)))
        .unwrap_or(MAX_DEPTH)
        .clamp(1, MAX_DEPTH)
}

fn time_budget(side: Color, params: &GoParams) -> Option<Duration> {
    if params.infinite {
        return None;
    }
    if let Some(move_time) = params.move_time_ms {
        return Some(Duration::from_millis(move_time));
    }

    let remaining = match side {
        Color::White => params.white_time_ms,
        Color::Black => params.black_time_ms,
    };
    let Some(remaining) = remaining else {
        if params.depth.is_some() || params.nodes.is_some() || params.mate.is_some() {
            return None;
        }
        return Some(Duration::from_millis(1_000));
    };

    let game_start = params.game_start_time_ms.unwrap_or(remaining);
    let divider = if remaining >= game_start / 3 {
        40
    } else if remaining >= game_start / 5 {
        100
    } else if remaining >= game_start / 20 {
        250
    } else {
        800
    };
    let budget = Duration::from_millis(game_start / divider);
    Some(budget)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_scores_put_the_best_move_first_for_each_side() {
        let moves = ["a2a3", "b2b3", "c2c3"].map(|m| m.parse().unwrap());
        let scores = [(20, moves[0]), (-10, moves[1]), (5, moves[2])];

        let mut white = scores;
        order_root_scores(&mut white, Color::White);
        assert_eq!(white.map(|(score, _)| score), [20, 5, -10]);

        let mut black = scores;
        order_root_scores(&mut black, Color::Black);
        assert_eq!(black.map(|(score, _)| score), [-10, 5, 20]);
    }

    #[test]
    fn repetition_preserves_uncapturable_en_passant_identity() {
        let mut s = Searcher::new(
            1,
            Arc::new(AtomicU64::new(0)),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        s.position_hashes = vec![(123, 4), (456, 0), (789, 0), (456, 0), (123, 0)];
        assert!(!s.is_repetition((123, 0), 4));
        s.position_hashes[0] = (123, 0);
        assert!(s.is_repetition((123, 0), 4));
        assert!(!s.is_repetition((123, 0), 0));
    }

    #[test]
    fn fixed_move_time_is_used_as_the_budget() {
        let params = GoParams {
            move_time_ms: Some(100),
            ..GoParams::default()
        };
        let budget = time_budget(Color::White, &params).unwrap();
        assert_eq!(budget, Duration::from_millis(100));
    }

    #[test]
    fn clock_budget_uses_time_divider_tiers() {
        let budget_at = |remaining| {
            time_budget(
                Color::White,
                &GoParams {
                    white_time_ms: Some(remaining),
                    game_start_time_ms: Some(10_000),
                    ..GoParams::default()
                },
            )
            .unwrap()
        };

        assert_eq!(budget_at(3_334), Duration::from_millis(250));
        assert_eq!(budget_at(3_332), Duration::from_millis(100));
        assert_eq!(budget_at(1_999), Duration::from_millis(40));
        assert_eq!(budget_at(499), Duration::from_millis(12));
    }

    #[test]
    fn search_depth_defaults_to_and_is_capped_at_forty() {
        assert_eq!(max_search_depth(&GoParams::default()), 40);
        assert_eq!(
            max_search_depth(&GoParams {
                depth: Some(80),
                ..GoParams::default()
            }),
            40
        );
    }

    #[test]
    fn finds_a_legal_move_at_depth_one() {
        let command_epoch = Arc::new(AtomicU64::new(0));
        let ponder_signal = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut searcher = Searcher::new(1, command_epoch, ponder_signal);
        let result = searcher.search(
            &Position::default(),
            &GoParams {
                depth: Some(1),
                ..GoParams::default()
            },
            0,
            |_| {},
        );
        assert!(result.best_move.is_some());
    }

    #[test]
    fn quiescence_searches_recaptures_at_the_horizon() {
        let command_epoch = Arc::new(AtomicU64::new(0));
        let ponder_signal = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut searcher = Searcher::new(1, command_epoch, ponder_signal);
        let position = Position::from_uci("position fen 3r3k/8/8/8/3Q4/8/8/K7 b - - 0 1").unwrap();
        searcher.position_hashes.clone_from(&position.hashes);

        let stand_pat = evaluate(&position.board);
        let score = searcher
            .quiescence_search(
                &position.board,
                1,
                i32::MIN,
                i32::MAX,
                (position.halfmove_clock, 0),
                1,
            )
            .unwrap();

        assert!(score < stand_pat - 500);
    }

    #[test]
    fn treats_quiet_checks_as_tactical_moves() {
        let searcher = Searcher::new(
            1,
            Arc::new(AtomicU64::new(0)),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let position = Position::from_uci("position fen 7k/8/8/8/8/8/R7/K7 w - - 0 1").unwrap();
        let checking_move: ChessMove = "a2a8".parse().unwrap();

        assert!(!is_capture(&position.board, checking_move));
        assert!(searcher.is_tactical_move(&position.board, checking_move));
    }

    #[test]
    fn stops_after_finding_a_winning_mate() {
        let command_epoch = Arc::new(AtomicU64::new(0));
        let ponder_signal = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut searcher = Searcher::new(1, command_epoch, ponder_signal);
        let position =
            Position::from_uci("position fen 6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1").unwrap();
        let mut info = None;
        let result = searcher.search(
            &position,
            &GoParams {
                depth: Some(3),
                ..GoParams::default()
            },
            0,
            |value| info = Some(value),
        );

        assert_eq!(result.best_move.unwrap().to_string(), "d1d8");
        let info = info.unwrap();
        assert_eq!(info.depth, 1);
        assert_eq!(info.score, MATE_SCORE - 1);
        assert_eq!(info.nodes, 1);
    }

    #[test]
    fn restricts_root_moves() {
        let command_epoch = Arc::new(AtomicU64::new(0));
        let ponder_signal = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut searcher = Searcher::new(1, command_epoch, ponder_signal);
        let only_move = "e2e4".parse().unwrap();
        let result = searcher.search(
            &Position::default(),
            &GoParams {
                search_moves: vec![only_move],
                depth: Some(1),
                ..GoParams::default()
            },
            0,
            |_| {},
        );
        assert_eq!(result.best_move, Some(only_move));
    }

    #[test]
    fn node_limit_remains_exact() {
        let command_epoch = Arc::new(AtomicU64::new(0));
        let ponder_signal = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut searcher = Searcher::new(1, command_epoch, ponder_signal);
        searcher.search(
            &Position::default(),
            &GoParams {
                depth: Some(5),
                nodes: Some(10),
                ..GoParams::default()
            },
            0,
            |_| {},
        );
        assert_eq!(searcher.nodes, 10);
    }
}
