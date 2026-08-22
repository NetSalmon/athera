#![no_std]
#![no_main]

const WIDTH: usize = 80;
const HEIGHT: usize = 30;
const GENERATIONS: usize = 120;
const FRAME_DELAY: usize = 2_000_000;

type Grid = [[bool; WIDTH]; HEIGHT];

#[unsafe(no_mangle)]
fn main() {
    let mut grid = [[false; WIDTH]; HEIGHT];
    let mut next = [[false; WIDTH]; HEIGHT];

    seed_glider_gun(&mut grid, 2, 4);
    seed_r_pentomino(&mut grid, 58, 13);

    for generation in 0..GENERATIONS {
        // Clear the screen and move the cursor to the upper-left corner.
        athera_userland::print!("\x1b[2J\x1b[H");
        athera_userland::println!("Conway's Game of Life - generation {generation}");
        print_grid(&grid);
        delay();

        next_generation(&grid, &mut next);
        core::mem::swap(&mut grid, &mut next);
    }
}

fn print_grid(grid: &Grid) {
    let mut line = [b' '; WIDTH * 2];

    for row in grid {
        for (column, alive) in row.iter().enumerate() {
            let offset = column * 2;
            if *alive {
                line[offset] = b'#';
                line[offset + 1] = b'#';
            } else {
                line[offset] = b' ';
                line[offset + 1] = b' ';
            }
        }

        // The fixed buffer avoids one write syscall per cell.
        athera_userland::println!("{}", core::str::from_utf8(&line).unwrap());
    }
}

fn seed_glider_gun(grid: &mut Grid, left: usize, top: usize) {
    // Gosper glider gun, using the standard 36 x 9 pattern.
    const CELLS: &[(usize, usize)] = &[
        (1, 5),
        (1, 6),
        (2, 5),
        (2, 6),
        (11, 3),
        (11, 4),
        (12, 2),
        (12, 6),
        (13, 1),
        (13, 7),
        (14, 1),
        (14, 7),
        (15, 4),
        (15, 5),
        (15, 6),
        (16, 2),
        (16, 6),
        (17, 3),
        (17, 4),
        (17, 5),
        (21, 1),
        (21, 2),
        (22, 1),
        (22, 2),
        (23, 3),
        (23, 4),
        (24, 3),
        (24, 4),
        (25, 4),
        (25, 5),
        (26, 4),
        (26, 6),
        (27, 6),
        (28, 6),
        (29, 5),
        (29, 7),
        (30, 4),
        (30, 5),
        (30, 6),
        (35, 3),
        (35, 4),
        (36, 3),
        (36, 4),
    ];

    for &(column, row) in CELLS {
        grid[top + row][left + column] = true;
    }
}

fn seed_r_pentomino(grid: &mut Grid, left: usize, top: usize) {
    // An R-pentomino expands chaotically before settling down.
    const CELLS: &[(usize, usize)] = &[(1, 0), (2, 0), (0, 1), (1, 1), (1, 2)];

    for &(column, row) in CELLS {
        grid[top + row][left + column] = true;
    }
}

fn next_generation(current: &Grid, next: &mut Grid) {
    for row in 0..HEIGHT {
        for column in 0..WIDTH {
            let neighbors = count_neighbors(current, row, column);
            next[row][column] = neighbors == 3 || (current[row][column] && neighbors == 2);
        }
    }
}

fn count_neighbors(grid: &Grid, row: usize, column: usize) -> u8 {
    let mut count = 0;

    let row_start = row.saturating_sub(1);
    let row_end = core::cmp::min(row + 1, HEIGHT - 1);
    let column_start = column.saturating_sub(1);
    let column_end = core::cmp::min(column + 1, WIDTH - 1);

    for neighbor_row in row_start..=row_end {
        for neighbor_column in column_start..=column_end {
            if (neighbor_row != row || neighbor_column != column)
                && grid[neighbor_row][neighbor_column]
            {
                count += 1;
            }
        }
    }

    count
}

fn delay() {
    for _ in 0..FRAME_DELAY {
        core::hint::spin_loop();
    }
}
