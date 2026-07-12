use const_val::const_val;

#[const_val]
const TEST_USIZE: usize = 42;

#[const_val]
const TEST_U64: u64 = 100;

#[const_val]
const TEST_U32: u32 = 2048;

#[const_val]
const TEST_U8: u8 = 255;

#[const_val]
const TEST_ISIZE: isize = -42;

#[const_val]
const TEST_I64: i64 = -100;

#[const_val]
const TEST_OVERRIDE: usize = 1;

#[test]
fn test_default_values() {
    assert_eq!(TEST_USIZE, 42);
    assert_eq!(TEST_U64, 100);
    assert_eq!(TEST_U32, 2048);
    assert_eq!(TEST_U8, 255);
    assert_eq!(TEST_ISIZE, -42);
    assert_eq!(TEST_I64, -100);
}

#[const_val]
const PAGE_SIZE: usize = 4096;

#[const_val]
const BUDDY_MAX_ORDER: usize = 11;

#[const_val]
const SLUB_MAX_ORDER: usize = 11;

#[const_val]
const SLUB_MIN_ORDER: usize = 4;

#[test]
fn test_novus_constants_default() {
    assert_eq!(PAGE_SIZE, 4096);
    assert_eq!(BUDDY_MAX_ORDER, 11);
    assert_eq!(SLUB_MAX_ORDER, 11);
    assert_eq!(SLUB_MIN_ORDER, 4);
}

#[test]
fn test_override_via_env() {
    // This test passes when TEST_OVERRIDE env var is set to 999
    // or when it's not set (defaults to 1)
    if option_env!("TEST_OVERRIDE").is_some() {
        assert_eq!(TEST_OVERRIDE, 999);
    } else {
        assert_eq!(TEST_OVERRIDE, 1);
    }
}

#[test]
fn test_hex_override() {
    #[const_val]
    const HEX_VAL: usize = 0;
    // When HEX_VAL=0xff is set, it should be 255
    if option_env!("HEX_VAL").is_some() {
        assert_eq!(HEX_VAL, 255);
    } else {
        assert_eq!(HEX_VAL, 0);
    }
}

#[test]
fn test_negative_override() {
    #[const_val]
    const NEG_VAL: isize = 0;
    if option_env!("NEG_VAL").is_some() {
        assert_eq!(NEG_VAL, -987);
    } else {
        assert_eq!(NEG_VAL, 0);
    }
}