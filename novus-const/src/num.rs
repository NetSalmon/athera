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
