use super::{MoveDirection, keyboard_move};

fn board() -> Vec<(String, Vec<i64>)> {
    vec![
        ("Backlog".to_string(), vec![1, 2]),
        ("Doing".to_string(), vec![3]),
        ("Other".to_string(), vec![4, 5]),
    ]
}

#[test]
fn moves_right_to_the_neighbour_column() {
    assert_eq!(
        keyboard_move(&board(), 1, MoveDirection::Right).as_deref(),
        Some("Doing")
    );
}

#[test]
fn moves_left_to_the_neighbour_column() {
    assert_eq!(
        keyboard_move(&board(), 3, MoveDirection::Left).as_deref(),
        Some("Backlog")
    );
}

#[test]
fn edge_moves_are_no_ops() {
    assert_eq!(keyboard_move(&board(), 2, MoveDirection::Left), None);
    assert_eq!(keyboard_move(&board(), 4, MoveDirection::Right), None);
}

#[test]
fn unknown_task_is_a_no_op() {
    assert_eq!(keyboard_move(&board(), 99, MoveDirection::Right), None);
}

#[test]
fn move_into_the_trailing_other_bucket_is_allowed() {
    assert_eq!(
        keyboard_move(&board(), 3, MoveDirection::Right).as_deref(),
        Some("Other")
    );
}
