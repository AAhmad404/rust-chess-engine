use chess::{
    Board, Color, EMPTY, File, Piece, Rank, Square, get_bishop_moves, get_knight_moves,
    get_pawn_attacks, get_rook_moves,
};

const PIECE_VALUES: [i32; 6] = [100, 300, 325, 500, 900, 0];
const MOBILITY_MULTIPLIER: i32 = 3;
const PIECE_TRADING_PENALTY: i32 = 15;
const IN_CHECK_PENALTY: i32 = 25;
const DOUBLED_PAWNS_PENALTY: i32 = 50;
const KING_SAFETY_PENALTY: i32 = 10;
const ROOKS_CONNECTED_BONUS: i32 = 40;
const ROOK_OPEN_FILE_BONUS: i32 = 30;
const ROOK_INFILTRATION_BONUS: i32 = 70;
const BISHOP_PAIR_BONUS: i32 = 15;

const PIECES: [Piece; 6] = [
    Piece::Pawn,
    Piece::Knight,
    Piece::Bishop,
    Piece::Rook,
    Piece::Queen,
    Piece::King,
];

const PAWN: [i32; 64] = [
    0, 0, 0, 0, 0, 0, 0, 0, 70, 70, 70, 70, 70, 70, 70, 70, 50, 50, 55, 55, 55, 55, 50, 50, 35, 35,
    40, 40, 40, 40, 35, 35, 25, 25, 30, 40, 40, 30, 25, 25, 15, 15, 20, 20, 20, 20, 15, 15, 25, 25,
    25, 0, 0, 25, 25, 25, 0, 0, 0, 0, 0, 0, 0, 0,
];

const KNIGHT: [i32; 64] = [
    -15, -10, -10, -10, -10, -10, -10, -15, -5, 0, 5, 5, 5, 5, 0, -5, -5, 5, 15, 10, 10, 15, 5, -5,
    -5, 5, 10, 10, 10, 10, 5, -5, -5, 5, 10, 10, 10, 10, 5, -5, -5, 5, 15, 10, 10, 15, 5, -5, -5,
    0, 5, 5, 5, 5, 0, -5, -15, -10, -10, -10, -10, -10, -10, -15,
];

const BISHOP: [i32; 64] = [
    15, -10, -10, -10, -10, -10, -10, 15, 0, 20, 5, 0, 0, 5, 20, 0, 0, 5, 15, 5, 5, 15, 5, 0, 0, 0,
    5, 15, 15, 5, 0, 0, 0, 0, 5, 15, 15, 5, 0, 0, 0, 5, 15, 5, 5, 15, 5, 0, 0, 20, 5, -5, -5, 5,
    20, 0, 15, -10, -10, -10, -10, -10, -10, 15,
];

const ROOK: [i32; 64] = [
    -10, -5, 0, 0, 0, 0, -5, -10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 5, 5, 5, 0, 0, 0, 0, 5, 10, 10,
    5, 0, 0, 0, 0, 5, 10, 10, 5, 0, 0, 0, 0, 5, 5, 5, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -10, -5, 0,
    0, 0, 0, -5, -10,
];

const QUEEN: [i32; 64] = [
    -10, 0, 0, 0, 0, 0, 0, -10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 5, 5, 5, 0, 0, 0, 0, 5, 10, 10, 5,
    0, 0, 0, 0, 5, 10, 10, 5, 0, 0, 0, 0, 5, 5, 5, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -10, 0, 0, 0,
    0, 0, 0, -10,
];

const KING: [i32; 64] = [
    0, 10, -3, -5, -5, -3, 10, 0, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5,
    -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -5,
    -5, -5, -5, -5, -5, -5, -5, -5, 0, 10, -3, -5, -5, -3, 10, 0,
];

const PAWN_ENDGAME: [i32; 64] = [
    0, 0, 0, 0, 0, 0, 0, 0, 80, 80, 80, 80, 80, 80, 80, 80, 50, 55, 60, 60, 60, 60, 55, 50, 30, 40,
    40, 40, 40, 40, 40, 30, 20, 20, 25, 25, 25, 25, 20, 20, 10, 10, 15, 15, 15, 15, 10, 10, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

const BISHOP_ENDGAME: [i32; 64] = [
    5, 0, 0, 0, 0, 0, 0, 5, 0, 5, 0, 0, 0, 0, 5, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 5, 0, 0, 0, 5, 0, 0, 0, 0, 5, 0, 5, 0, 0, 0, 0, 0, 0, 5,
];

const KING_ENDGAME: [i32; 64] = [
    -25, -10, -5, -5, -5, -5, -1, -25, -10, 0, 0, 0, 0, 0, 0, -10, -5, 0, 5, 10, 10, 5, 0, -5, -5,
    0, 10, 15, 15, 10, 0, -5, -5, 0, 10, 15, 15, 10, 0, -5, -5, 0, 5, 10, 10, 5, 0, -5, -10, 0, 0,
    0, 0, 0, 0, -10, -25, -10, -5, -5, -5, -5, -10, -25,
];

#[must_use]
pub fn evaluate(board: &Board) -> i32 {
    // Positive scores favor White; search converts this to the side-to-move perspective.
    let endgame = is_endgame(board);
    let mut white = side_score(board, Color::White, endgame);
    let mut black = side_score(board, Color::Black, endgame);

    if board.checkers().popcnt() != 0 {
        match board.side_to_move() {
            Color::White => white -= IN_CHECK_PENALTY,
            Color::Black => black -= IN_CHECK_PENALTY,
        }
    }

    let white_king = board.king_square(Color::White);
    let black_king = board.king_square(Color::Black);
    if endgame {
        white -= active_king_penalty(white_king);
        black -= active_king_penalty(black_king);

        let file_delta =
            white_king.get_file().to_index() as f64 - black_king.get_file().to_index() as f64;
        let rank_delta =
            white_king.get_rank().to_index() as f64 - black_king.get_rank().to_index() as f64;
        let distance = (file_delta * file_delta + rank_delta * rank_delta)
            .sqrt()
            .ceil() as i32;
        if white > black {
            white -= distance * 10;
        }
        if black > white {
            black -= distance * 10;
        }
    } else {
        white += king_safety(board, Color::White);
        black += king_safety(board, Color::Black);
    }

    let white_count = board.color_combined(Color::White).popcnt() as i32;
    let black_count = board.color_combined(Color::Black).popcnt() as i32;
    if white < black && white_count < black_count {
        white -= PIECE_TRADING_PENALTY;
    }
    if black < white && black_count < white_count {
        black -= PIECE_TRADING_PENALTY;
    }

    white - black
}

#[must_use]
pub fn evaluate_for_side_to_move(board: &Board) -> i32 {
    let score = evaluate(board);
    if board.side_to_move() == Color::White {
        score
    } else {
        -score
    }
}

fn side_score(board: &Board, color: Color, endgame: bool) -> i32 {
    let mut score = 0;
    for piece in PIECES {
        let squares = *board.pieces(piece) & *board.color_combined(color);
        for square in squares {
            score += PIECE_VALUES[piece.to_index()];
            score += piece_square_value(piece, square, color, endgame);
            if piece != Piece::King {
                score += MOBILITY_MULTIPLIER * attack_count(board, piece, square, color);
            }
            if piece == Piece::Rook {
                if rook_has_open_file(board, square) {
                    score += ROOK_OPEN_FILE_BONUS;
                }
                let rank = square.get_rank().to_index();
                if (color == Color::White && rank == 6) || (color == Color::Black && rank == 1) {
                    score += ROOK_INFILTRATION_BONUS;
                }
            }
        }
    }

    score += doubled_pawn_penalty(board, color);
    if (*board.pieces(Piece::Bishop) & *board.color_combined(color)).popcnt() == 2 {
        score += BISHOP_PAIR_BONUS;
    }
    if rooks_connected(board, color) {
        score += ROOKS_CONNECTED_BONUS;
    }
    score
}

fn piece_square_value(piece: Piece, square: Square, color: Color, endgame: bool) -> i32 {
    let index = match (color, piece) {
        (Color::White, Piece::Pawn) => 63 - square.to_index(),
        _ => square.to_index(),
    };
    match (piece, endgame) {
        (Piece::Pawn, true) => PAWN_ENDGAME[index],
        (Piece::Bishop, true) => BISHOP_ENDGAME[index],
        (Piece::Rook, true) => 0,
        (Piece::King, true) => KING_ENDGAME[index],
        (Piece::Pawn, false) => PAWN[index],
        (Piece::Knight, _) => KNIGHT[index],
        (Piece::Bishop, false) => BISHOP[index],
        (Piece::Rook, false) => ROOK[index],
        (Piece::Queen, _) => QUEEN[index],
        (Piece::King, false) => KING[index],
    }
}

fn is_endgame(board: &Board) -> bool {
    for color in [Color::White, Color::Black] {
        let own = *board.color_combined(color);
        if (*board.pieces(Piece::Queen) & own).popcnt() != 0 {
            return false;
        }
        let remaining = ((*board.pieces(Piece::Bishop)
            | *board.pieces(Piece::Knight)
            | *board.pieces(Piece::Rook))
            & own)
            .popcnt();
        if remaining > 3 {
            return false;
        }
    }
    true
}

fn doubled_pawn_penalty(board: &Board, color: Color) -> i32 {
    let pawns = *board.pieces(Piece::Pawn) & *board.color_combined(color);
    let mut counts = [0_u8; 8];
    for square in pawns {
        counts[square.get_file().to_index()] += 1;
    }
    counts
        .iter()
        .map(|count| i32::from(count.saturating_sub(1)) * -DOUBLED_PAWNS_PENALTY)
        .sum()
}

fn king_safety(board: &Board, color: Color) -> i32 {
    let king = board.king_square(color);
    let file = king.get_file().to_index() as i32;
    let rank = king.get_rank().to_index() as i32;
    let forward = if color == Color::White { 1 } else { -1 };
    let mut score = 0;

    for file_offset in -1..=1 {
        let target_file = file + file_offset;
        let target_rank = rank + forward;
        if !(0..8).contains(&target_file) || !(0..8).contains(&target_rank) {
            score -= KING_SAFETY_PENALTY;
            continue;
        }
        let square = make_square(target_file, target_rank);
        if board.piece_on(square) != Some(Piece::Pawn) || board.color_on(square) != Some(color) {
            score -= KING_SAFETY_PENALTY;
        }
    }
    score
}

fn active_king_penalty(square: Square) -> i32 {
    let file = square.get_file().to_index() as i32;
    let rank = square.get_rank().to_index() as i32;
    let file_distance = (file - 3).abs().min((file - 4).abs());
    let rank_distance = (rank - 3).abs().min((rank - 4).abs());
    (((file_distance + rank_distance) * 10) as f64).powf(1.1) as i32
}

fn rook_has_open_file(board: &Board, rook: Square) -> bool {
    let file = rook.get_file().to_index() as i32;
    let rank = rook.get_rank().to_index() as i32;
    let mut empty = 0;
    for direction in [-1, 1] {
        let mut target = rank + direction;
        while (0..8).contains(&target) {
            let square = make_square(file, target);
            if board.piece_on(square).is_some()
                && !(board.piece_on(square) == Some(Piece::Rook)
                    && board.color_on(square) == board.color_on(rook))
            {
                break;
            }
            empty += 1;
            target += direction;
        }
    }
    empty >= 6
}

fn rooks_connected(board: &Board, color: Color) -> bool {
    let rooks: Vec<Square> = (*board.pieces(Piece::Rook) & *board.color_combined(color))
        .into_iter()
        .collect();
    if rooks.len() != 2 {
        return false;
    }

    let a_file = rooks[0].get_file().to_index() as i32;
    let a_rank = rooks[0].get_rank().to_index() as i32;
    let b_file = rooks[1].get_file().to_index() as i32;
    let b_rank = rooks[1].get_rank().to_index() as i32;

    let (df, dr) = if a_rank == b_rank {
        ((b_file - a_file).signum(), 0)
    } else if a_file == b_file {
        (0, (b_rank - a_rank).signum())
    } else {
        return false;
    };

    let mut file = a_file + df;
    let mut rank = a_rank + dr;
    while file != b_file || rank != b_rank {
        if board.piece_on(make_square(file, rank)).is_some() {
            return false;
        }
        file += df;
        rank += dr;
    }
    true
}

fn attack_count(board: &Board, piece: Piece, square: Square, color: Color) -> i32 {
    // Mobility counts attacked squares, including the first occupied square on each ray.
    match piece {
        Piece::Pawn => get_pawn_attacks(square, color, !EMPTY).popcnt() as i32,
        Piece::Knight => get_knight_moves(square).popcnt() as i32,
        Piece::Bishop => get_bishop_moves(square, *board.combined()).popcnt() as i32,
        Piece::Rook => get_rook_moves(square, *board.combined()).popcnt() as i32,
        Piece::Queen => (get_bishop_moves(square, *board.combined())
            | get_rook_moves(square, *board.combined()))
        .popcnt() as i32,
        Piece::King => 0,
    }
}

fn make_square(file: i32, rank: i32) -> Square {
    Square::make_square(
        Rank::from_index(rank as usize),
        File::from_index(file as usize),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn initial_position_is_equal() {
        assert_eq!(evaluate(&Board::default()), 0);
    }

    #[test]
    fn extra_white_queen_is_positive() {
        let board = Board::from_str("4k3/8/8/8/8/8/3Q4/4K3 w - - 0 1").unwrap();
        assert!(evaluate(&board) > 800);
    }
}
