use super::*;

#[test]
fn a_caught_one_is_the_shell_s_number_and_a_tree_gets_all_but_the_interrupt() {
    assert_eq!(Caught::Interrupt.number(), 130);
    assert_eq!(Caught::Terminate.number(), 143);
    assert_eq!(Caught::Hangup.number(), 129);
    assert_eq!(Caught::Interrupt.forward(), None, "the terminal sent it");
    assert_eq!(Caught::Terminate.forward(), Some(Signal::Terminate));
    assert_eq!(Caught::Hangup.forward(), Some(Signal::Hangup));
}

#[tokio::test]
async fn nothing_listened_for_is_never_caught() {
    let mut none = Interrupts::listen(&[]).unwrap();
    assert_eq!(none.already().await, None);
}
