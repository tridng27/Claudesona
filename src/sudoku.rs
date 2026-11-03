// "Sudoku": a 9x9 puzzle made locally from a seed. You click a cell, then a number, because her window never
// takes keyboard focus. Any complete grid with no clashes wins, so the puzzle needn't have a unique solution.

const BLANKS: usize = 45; // cells to fill in: an easy-to-medium puzzle

pub struct Sudoku {
    pub cells: [u8; 81], // 0 = empty
    pub given: [bool; 81],
    pub selected: Option<usize>,
}

impl Sudoku {
    pub fn new(seed: u32) -> Self {
        let mut rng = seed | 1;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            rng
        };
        let mut cells = [0; 81];
        fill(&mut cells, &mut next);
        // Blank BLANKS cells picked by a shuffle.
        let mut order: Vec<usize> = (0..81).collect();
        for i in (1..81).rev() {
            order.swap(i, next() as usize % (i + 1));
        }
        for &i in &order[..BLANKS] {
            cells[i] = 0;
        }
        Sudoku { cells, given: cells.map(|c| c != 0), selected: None }
    }

    /// Puts `v` (0 erases) in the selected cell, unless it's a given or the puzzle is solved.
    pub fn put(&mut self, v: u8) {
        if let Some(i) = self.selected.filter(|&i| !self.given[i] && !self.won()) {
            self.cells[i] = v;
        }
    }

    /// The cell holds a number that also sits in its row, column or box.
    pub fn clash(&self, i: usize) -> bool {
        self.cells[i] != 0 && (0..81).any(|j| j != i && self.cells[j] == self.cells[i] && peers(i, j))
    }

    pub fn won(&self) -> bool {
        (0..81).all(|i| self.cells[i] != 0 && !self.clash(i))
    }
}

fn peers(i: usize, j: usize) -> bool {
    i / 9 == j / 9 || i % 9 == j % 9 || (i / 27 == j / 27 && i % 9 / 3 == j % 9 / 3)
}

/// Fills the empty cells with a valid grid by backtracking, trying digits in random order.
fn fill(cells: &mut [u8; 81], next: &mut impl FnMut() -> u32) -> bool {
    let Some(i) = cells.iter().position(|&c| c == 0) else { return true };
    let start = next() as u8 % 9;
    for k in 0..9 {
        let v = (start + k) % 9 + 1;
        if (0..81).all(|j| cells[j] != v || !peers(i, j)) {
            cells[i] = v;
            if fill(cells, next) {
                return true;
            }
        }
    }
    cells[i] = 0;
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_a_solvable_puzzle_and_spots_clashes() {
        let mut s = Sudoku::new(42);
        assert_eq!(s.cells.iter().filter(|&&c| c == 0).count(), BLANKS);
        assert!((0..81).all(|i| !s.clash(i)));
        // Refill it from a full grid made the same way: that solves it.
        let mut full = s.cells;
        assert!(fill(&mut full, &mut || 7));
        let blank = (0..81).find(|&i| !s.given[i]).unwrap();
        let given = (0..81).find(|&i| s.given[i]).unwrap();
        s.selected = Some(given);
        s.put(full[given] % 9 + 1);
        assert_eq!(s.cells[given], full[given]); // givens can't change
        // A number already in that blank cell's row clashes.
        let row_mate = (0..81).find(|&j| j / 9 == blank / 9 && s.given[j]).unwrap();
        s.selected = Some(blank);
        s.put(s.cells[row_mate]);
        assert!(s.clash(blank) && !s.won());
        for i in 0..81 {
            if !s.given[i] {
                s.selected = Some(i);
                s.put(full[i]);
            }
        }
        assert!(s.won());
        s.selected = Some(blank);
        s.put(0);
        assert_ne!(s.cells[blank], 0); // no changes once solved
    }
}
