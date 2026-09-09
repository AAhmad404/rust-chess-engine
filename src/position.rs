use chess::{Board, ChessMove, Piece};
use std::str::FromStr;

#[derive(Clone)]
pub struct Position {
    pub board: Board,
    pub hashes: Vec<(u64, u8)>,
    pub halfmove_clock: u16,
}

impl Default for Position {
    fn default() -> Self {
        let board = Board::default();
        Self {
            board,
            hashes: vec![(board.get_hash(), 0)],
            halfmove_clock: 0,
        }
    }
}

impl Position {
    pub fn from_uci(command: &str) -> Result<Self, String> {
        let tokens: Vec<&str> = command.split_whitespace().collect();
        if tokens.first() != Some(&"position") {
            return Err("expected a position command".into());
        }

        let moves_index = tokens.iter().position(|token| *token == "moves");
        let setup_end = moves_index.unwrap_or(tokens.len());

        let (board, halfmove_clock, ep) = match tokens.get(1).copied() {
            Some("startpos") => (Board::default(), 0, 0),
            Some("fen") => {
                if setup_end < 8 {
                    return Err("FEN must contain six fields".into());
                }
                let fen_fields = &tokens[2..setup_end];
                if fen_fields.len() != 6 {
                    return Err("FEN must contain six fields".into());
                }
                let fen = fen_fields.join(" ");
                let board = Board::from_str(&fen).map_err(|_| "invalid FEN".to_string())?;
                let halfmove = fen_fields[4]
                    .parse::<u16>()
                    .map_err(|_| "invalid FEN halfmove clock".to_string())?;
                let ep = if fen_fields[3] == "-" {
                    0
                } else {
                    let file = fen_fields[3].as_bytes()[0];
                    if !(b'a'..=b'h').contains(&file) {
                        return Err("invalid en passant file".into());
                    }
                    file - b'a' + 1
                };
                (board, halfmove, ep)
            }
            _ => return Err("expected 'startpos' or 'fen'".into()),
        };

        let mut position = Self {
            board,
            hashes: vec![(board.get_hash(), ep)],
            halfmove_clock,
        };

        if let Some(index) = moves_index {
            for text in &tokens[index + 1..] {
                let chess_move =
                    ChessMove::from_str(text).map_err(|_| format!("invalid move: {text}"))?;
                position.play(chess_move)?;
            }
        }

        Ok(position)
    }

    pub fn play(&mut self, chess_move: ChessMove) -> Result<(), String> {
        if !self.board.legal(chess_move) {
            return Err(format!("illegal move: {chess_move}"));
        }

        self.halfmove_clock = next_halfmove_clock(&self.board, chess_move, self.halfmove_clock);
        let ep = ep_file(&self.board, chess_move);
        self.board = self.board.make_move_new(chess_move);
        self.hashes.push((self.board.get_hash(), ep));
        Ok(())
    }
}

pub fn ep_file(board: &Board, m: ChessMove) -> u8 {
    if board.piece_on(m.get_source()) == Some(Piece::Pawn)
        && m.get_source().to_index().abs_diff(m.get_dest().to_index()) == 16
    {
        m.get_source().get_file().to_index() as u8 + 1
    } else {
        0
    }
}

pub fn is_capture(board: &Board, chess_move: ChessMove) -> bool {
    if board.piece_on(chess_move.get_dest()).is_some() {
        return true;
    }

    // A diagonal pawn move to an empty destination is an en passant capture.
    board.piece_on(chess_move.get_source()) == Some(Piece::Pawn)
        && chess_move.get_source().get_file() != chess_move.get_dest().get_file()
}

pub fn next_halfmove_clock(board: &Board, chess_move: ChessMove, current: u16) -> u16 {
    if board.piece_on(chess_move.get_source()) == Some(Piece::Pawn) || is_capture(board, chess_move)
    {
        0
    } else {
        current.saturating_add(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_start_position_moves() {
        let position = Position::from_uci("position startpos moves e2e4 e7e5 g1f3").unwrap();
        assert_eq!(position.hashes.len(), 4);
        assert_eq!(position.halfmove_clock, 1);
    }

    #[test]
    fn rejects_illegal_moves() {
        assert!(Position::from_uci("position startpos moves e2e5").is_err());
    }
}
