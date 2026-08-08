//! 编译期常量字符串解析。
//!
//! [`parse_digit_*`](parse_digit_usize) 系列 `const fn` 把字符串按
//! 十进制 / 十六进制（`0x`）/ 八进制（`0o`）/ 二进制（`0b`）解析为
//! 整数；[`parse_bool`] 解析布尔值。均供 `#[const_val]` 宏在编译期求值。
pub enum NumberBase {
    Decimal,
    Hexadecimal,
    Octal,
    Binary,
}

pub enum Fsm {
    Start,
    Sign,
    LeadingZero,
    Base,
    Number,
}

macro_rules! parse_digit {
    ($t:ty) => {
        paste::paste! {
            pub const fn [<parse_digit_ $t >](source: &str, default: $t) -> $t {
                if source.is_empty() { return default; }
                let source = source.as_bytes();
                let mut status = Fsm::Start;

                let mut result = 0;
                let mut form = NumberBase::Decimal;

                let mut i = 0;
                while i < source.len() {
                    match (source[i], &status) {
                        (b'+', Fsm::Sign) => {
                            status = Fsm::Sign;
                        }
                        (b'0', Fsm::Sign | Fsm::Start) => {
                            status = Fsm::LeadingZero;
                        }
                        (b'x' | b'X', Fsm::LeadingZero) => {
                            status = Fsm::Base;
                            form = NumberBase::Hexadecimal;
                        }
                        (b'b' | b'B', Fsm::LeadingZero) => {
                            status = Fsm::Base;
                            form = NumberBase::Binary;
                        }
                        (b'o' | b'O', Fsm::LeadingZero) => {
                            status = Fsm::Base;
                            form = NumberBase::Octal;
                        }
                        (b'0'..=b'1', Fsm::Base | Fsm::Number | Fsm::LeadingZero | Fsm::Sign | Fsm::Start) => {
                            status = Fsm::Number;
                            match form {
                                NumberBase::Decimal => { result = result * 10 + (source[i] - b'0') as $t; }
                                NumberBase::Hexadecimal => { result = result * 16 + (source[i] - b'0') as $t; }
                                NumberBase::Octal => { result = result * 8 + (source[i] - b'0') as $t; }
                                NumberBase::Binary => { result = result * 2 + (source[i] - b'0') as $t; }
                            }
                        }
                        (b'0'..=b'7', Fsm::Base | Fsm::Number | Fsm::LeadingZero | Fsm::Sign | Fsm::Start) => {
                            status = Fsm::Number;
                            match form {
                                NumberBase::Decimal => { result = result * 10 + (source[i] - b'0') as $t; }
                                NumberBase::Hexadecimal => { result = result * 16 + (source[i] - b'0') as $t; }
                                NumberBase::Octal => { result = result * 8 + (source[i] - b'0') as $t; }
                                _ => { return default }
                            }
                        }
                        (b'0'..=b'9', Fsm::Base | Fsm::Number | Fsm::LeadingZero | Fsm::Sign | Fsm::Start) => {
                            status = Fsm::Number;
                            match form {
                                NumberBase::Decimal => { result = result * 10 + (source[i] - b'0') as $t; }
                                NumberBase::Hexadecimal => { result = result * 16 + (source[i] - b'0') as $t; }
                                _ => { return default }
                            }
                        }
                        (b'a'..=b'f', Fsm::Base | Fsm::Number | Fsm::LeadingZero | Fsm::Sign | Fsm::Start) => {
                            status = Fsm::Number;
                            match form {
                                NumberBase::Hexadecimal => { result = result * 16 + (source[i] - b'a' + 10) as $t; }
                                _ => { return default }
                            }
                        }
                        (b'A'..=b'F', Fsm::Base | Fsm::Number | Fsm::LeadingZero | Fsm::Sign | Fsm::Start) => {
                            status = Fsm::Number;
                            match form {
                                NumberBase::Hexadecimal => { result = result * 16 + (source[i] - b'A' + 10) as $t; }
                                _ => { return default }
                            }
                        }
                        _ => return default,
                    }

                    i += 1;
                }

                result
            }
        }
    };
    (signed $t:ty) => {
        paste::paste! {
            pub const fn [<parse_digit_ $t >](source: &str, default: $t) -> $t {
                if source.is_empty() { return default; }
                let source = source.as_bytes();
                let mut status = Fsm::Start;

                let mut result = 0;
                let mut is_neg = false;
                let mut form = NumberBase::Decimal;

                let mut i = 0;
                while i < source.len() {
                    match (source[i], &status) {
                        (b'-', Fsm::Start) => {
                            is_neg = true;
                            status = Fsm::Sign;
                        }
                        (b'+', Fsm::Sign) => {
                            is_neg = false;
                            status = Fsm::Sign;
                        }
                        (b'0', Fsm::Sign | Fsm::Start) => {
                            status = Fsm::LeadingZero;
                        }
                        (b'x' | b'X', Fsm::LeadingZero) => {
                            status = Fsm::Base;
                            form = NumberBase::Hexadecimal;
                        }
                        (b'b' | b'B', Fsm::LeadingZero) => {
                            status = Fsm::Base;
                            form = NumberBase::Binary;
                        }
                        (b'o' | b'O', Fsm::LeadingZero) => {
                            status = Fsm::Base;
                            form = NumberBase::Octal;
                        }
                        (b'0'..=b'1', Fsm::Base | Fsm::Number | Fsm::LeadingZero | Fsm::Sign | Fsm::Start) => {
                            status = Fsm::Number;
                            match form {
                                NumberBase::Decimal => { result = result * 10 + (source[i] - b'0') as $t; }
                                NumberBase::Hexadecimal => { result = result * 16 + (source[i] - b'0') as $t; }
                                NumberBase::Octal => { result = result * 8 + (source[i] - b'0') as $t; }
                                NumberBase::Binary => { result = result * 2 + (source[i] - b'0') as $t; }
                            }
                        }
                        (b'0'..=b'7', Fsm::Base | Fsm::Number | Fsm::LeadingZero | Fsm::Sign | Fsm::Start) => {
                            status = Fsm::Number;
                            match form {
                                NumberBase::Decimal => { result = result * 10 + (source[i] - b'0') as $t; }
                                NumberBase::Hexadecimal => { result = result * 16 + (source[i] - b'0') as $t; }
                                NumberBase::Octal => { result = result * 8 + (source[i] - b'0') as $t; }
                                _ => { return default }
                            }
                        }
                        (b'0'..=b'9', Fsm::Base | Fsm::Number | Fsm::LeadingZero | Fsm::Sign | Fsm::Start) => {
                            status = Fsm::Number;
                            match form {
                                NumberBase::Decimal => { result = result * 10 + (source[i] - b'0') as $t; }
                                NumberBase::Hexadecimal => { result = result * 16 + (source[i] - b'0') as $t; }
                                _ => { return default }
                            }
                        }
                        (b'a'..=b'f', Fsm::Base | Fsm::Number | Fsm::LeadingZero | Fsm::Sign | Fsm::Start) => {
                            status = Fsm::Number;
                            match form {
                                NumberBase::Hexadecimal => { result = result * 16 + (source[i] - b'a' + 10) as $t; }
                                _ => { return default }
                            }
                        }
                        (b'A'..=b'F', Fsm::Base | Fsm::Number | Fsm::LeadingZero | Fsm::Sign | Fsm::Start) => {
                            status = Fsm::Number;
                            match form {
                                NumberBase::Hexadecimal => { result = result * 16 + (source[i] - b'A' + 10) as $t; }
                                _ => { return default }
                            }
                        }
                        _ => return default,
                    }

                    i += 1;
                }

                if is_neg { -result } else { result }
            }
        }
    };

}

parse_digit!(u8);
parse_digit!(u16);
parse_digit!(u32);
parse_digit!(u64);
parse_digit!(u128);
parse_digit!(usize);

parse_digit!(signed i8);
parse_digit!(signed i16);
parse_digit!(signed i32);
parse_digit!(signed i64);
parse_digit!(signed i128);
parse_digit!(signed isize);

/// 把字符串解析为布尔值，供 `#[const_val]` 在编译期求值。
///
/// 接受大小写不敏感的 `true` / `false` 与 `1` / `0`；无法识别时返回
/// `default`（与数字解析失败时回退默认值的行为一致）。
pub const fn parse_bool(source: &str, default: bool) -> bool {
    let bytes = source.as_bytes();
    if source.eq_ignore_ascii_case("true") || (bytes.len() == 1 && bytes[0] == b'1') {
        true
    } else if source.eq_ignore_ascii_case("false") || (bytes.len() == 1 && bytes[0] == b'0') {
        false
    } else {
        default
    }
}
