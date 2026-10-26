//! `worker.package_registries`

use super::{EXAMPLE, parse};

#[test]
fn it_is_on_in_the_example_and_when_absent_and_off_when_set_off() {
    let absent = EXAMPLE.replace("package_registries = true\n", "");
    let off = EXAMPLE.replace("package_registries = true", "package_registries = false");

    assert!(parse(EXAMPLE).unwrap().worker.package_registries);
    assert!(parse(&absent).unwrap().worker.package_registries);
    assert!(!parse(&off).unwrap().worker.package_registries);
}
