#[macro_export]
macro_rules! sign_extend {
    ($result_type:ty, $x:expr, $bitcount:expr) => {{
        i64::from($x)
            .wrapping_shl(64 - $bitcount)
            .wrapping_shr(64 - $bitcount) as $result_type
    }};

    ($x:expr, $bitcount:expr) => {
        sign_extend!(_, $x, $bitcount)
    };
}