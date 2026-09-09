use crate::position::is_capture;
use chess::{Board, ChessMove, Color, Piece};

pub fn move_key(board: &Board, m: ChessMove) -> u32 {
    let piece = board.piece_on(m.get_source()).unwrap();
    let capture = if is_capture(board, m) {
        board
            .piece_on(m.get_dest())
            .unwrap_or(Piece::Pawn)
            .to_index() as u32
            + 1
    } else {
        0
    };
    let flag = if let Some(p) = m.get_promotion() {
        p.to_index() as u32 + 3
    } else if piece == Piece::King
        && m.get_source().to_index().abs_diff(m.get_dest().to_index()) == 2
    {
        2
    } else if piece == Piece::Pawn
        && m.get_source().to_index().abs_diff(m.get_dest().to_index()) == 16
    {
        3
    } else if capture != 0 && board.piece_on(m.get_dest()).is_none() {
        1
    } else {
        0
    };
    m.get_source().to_index() as u32
        | ((m.get_dest().to_index() as u32) << 6)
        | (flag << 12)
        | (((piece.to_index() as u32 + 1) | capture << 3) << 16)
}

pub(crate) fn generation_key(board: &Board, m: ChessMove) -> (u8, usize, usize, u8) {
    let from = m.get_source().to_index();
    let to = m.get_dest().to_index();
    let df = (from % 8).abs_diff(to % 8);
    let dr = (from / 8).abs_diff(to / 8);
    let group = match board.piece_on(m.get_source()).unwrap() {
        Piece::King => {
            if df == 2 {
                if to % 8 == 6 { 1 } else { 2 }
            } else {
                0
            }
        }
        Piece::Rook => 3,
        Piece::Bishop => 4,
        Piece::Queen => {
            if df == 0 || dr == 0 {
                3
            } else {
                4
            }
        }
        Piece::Knight => 5,
        Piece::Pawn => {
            let delta = (to as i32 - from as i32)
                * if board.side_to_move() == Color::White {
                    1
                } else {
                    -1
                };
            if df != 0 && board.piece_on(m.get_dest()).is_none() {
                13
            } else if m.get_promotion().is_some() {
                match delta {
                    8 => 10,
                    7 => 11,
                    _ => 12,
                }
            } else {
                match delta {
                    8 => 6,
                    16 => 7,
                    7 => 8,
                    _ => 9,
                }
            }
        }
    };
    let promotion = match m.get_promotion() {
        Some(Piece::Queen) => 0,
        Some(Piece::Knight) => 1,
        Some(Piece::Rook) => 2,
        Some(Piece::Bishop) => 3,
        _ => 0,
    };
    if group >= 6 {
        (group, to, from, promotion)
    } else {
        (group, from, to, promotion)
    }
}

pub fn insufficient_material(board: &Board) -> bool {
    if (board.pieces(Piece::Pawn) | board.pieces(Piece::Rook) | board.pieces(Piece::Queen)).popcnt()
        != 0
    {
        return false;
    }
    let minors = (board.pieces(Piece::Knight) | board.pieces(Piece::Bishop)).popcnt();
    if minors <= 1 {
        return true;
    }
    let white = *board.pieces(Piece::Bishop) & *board.color_combined(Color::White);
    let black = *board.pieces(Piece::Bishop) & *board.color_combined(Color::Black);
    if minors == 2 && white.popcnt() == 1 && black.popcnt() == 1 {
        let a = white.to_square().to_index();
        let b = black.to_square().to_index();
        return (a / 8 + a % 8) % 2 == (b / 8 + b % 8) % 2;
    }
    false
}

// Equal scores retain the permutation produced by the paired-array introsort.
pub fn sort_root(items: &mut [(i32, ChessMove)]) {
    let depth = 2 * (usize::BITS - items.len().leading_zeros()) as usize;
    intro(items, depth);
}

fn swap_if(items: &mut [(i32, ChessMove)], a: usize, b: usize) {
    if items[a].0 > items[b].0 {
        items.swap(a, b);
    }
}

fn intro(mut items: &mut [(i32, ChessMove)], mut depth: usize) {
    while items.len() > 1 {
        let n = items.len();
        if n <= 16 {
            if n == 2 {
                swap_if(items, 0, 1);
                return;
            }
            if n == 3 {
                swap_if(items, 0, 1);
                swap_if(items, 0, 2);
                swap_if(items, 1, 2);
                return;
            }
            for i in 1..n {
                let item = items[i];
                let mut j = i;
                while j > 0 && item.0 < items[j - 1].0 {
                    items[j] = items[j - 1];
                    j -= 1;
                }
                items[j] = item;
            }
            return;
        }
        if depth == 0 {
            heap_sort(items);
            return;
        }
        depth -= 1;
        let middle = (n - 1) / 2;
        swap_if(items, 0, middle);
        swap_if(items, 0, n - 1);
        swap_if(items, middle, n - 1);
        let pivot = items[middle].0;
        items.swap(middle, n - 2);
        let mut left = 0;
        let mut right = n - 2;
        loop {
            left += 1;
            while items[left].0 < pivot {
                left += 1;
            }
            right -= 1;
            while pivot < items[right].0 {
                right -= 1;
            }
            if left >= right {
                break;
            }
            items.swap(left, right);
        }
        items.swap(left, n - 2);
        intro(&mut items[left + 1..], depth);
        items = &mut items[..left];
    }
}

fn heap_sort(items: &mut [(i32, ChessMove)]) {
    fn down(items: &mut [(i32, ChessMove)], mut i: usize, n: usize) {
        let item = items[i - 1];
        while i <= n / 2 {
            let mut child = 2 * i;
            if child < n && items[child - 1].0 < items[child].0 {
                child += 1;
            }
            if item.0 >= items[child - 1].0 {
                break;
            }
            items[i - 1] = items[child - 1];
            i = child;
        }
        items[i - 1] = item;
    }
    let n = items.len();
    for i in (1..=n / 2).rev() {
        down(items, i, n);
    }
    for i in (2..=n).rev() {
        items.swap(0, i - 1);
        down(items, 1, i - 1);
    }
}
