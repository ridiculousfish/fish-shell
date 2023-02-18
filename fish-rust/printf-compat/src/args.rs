use std::rc::Rc;
pub use widestring::{Utf32Str as wstr, Utf32String as WString};

/// Integer widths.
#[derive(Debug, Copy, Clone)]
pub enum IntWidth {
    W8,
    W16,
    W32,
    W64,
}

fn width_of<T>() -> IntWidth {
    match std::mem::size_of::<T>() {
        1 => IntWidth::W8,
        2 => IntWidth::W16,
        4 => IntWidth::W32,
        8 => IntWidth::W64,
        _ => panic!("Unrecognized width "),
    }
}

/// Printf argument types.
#[derive(Debug, Clone)]
pub enum Arg<'a> {
    Str(&'a wstr),
    BoxedStr(Rc<Box<wstr>>), // owning variant when passing in a UTF8 string, Rc for clone.
    Int(i64, IntWidth),
    UInt(u64, IntWidth),
    Float(f64),
    Char(char),
}

/// Conversion from a raw value to a printf argument.
pub trait ToArg<'a>: Copy {
    fn to_arg(self) -> Arg<'a>;
}

impl<'a> ToArg<'a> for &'a str {
    fn to_arg(self) -> Arg<'a> {
        Arg::BoxedStr(Rc::new(WString::from_str(self).into_boxed_utfstr()))
    }
}

impl<'a> ToArg<'a> for &'a wstr {
    fn to_arg(self) -> Arg<'a> {
        Arg::Str(self)
    }
}

impl ToArg<'static> for f32 {
    fn to_arg(self) -> Arg<'static> {
        Arg::Float(self as f64)
    }
}

impl ToArg<'static> for f64 {
    fn to_arg(self) -> Arg<'static> {
        Arg::Float(self)
    }
}

impl ToArg<'static> for char {
    fn to_arg(self) -> Arg<'static> {
        Arg::Char(self)
    }
}

/// All signed types.
macro_rules! impl_to_arg {
    ($($t:ty),*) => {
        $(
            impl ToArg<'static> for $t {
                fn to_arg(self) -> Arg<'static> {
                    Arg::Int(self as i64, width_of::<$t>())
                }
            }
        )*
    };
}
impl_to_arg!(i8, i16, i32, i64, isize);

/// All unsigned types.
macro_rules! impl_to_arg_u {
    ($($t:ty),*) => {
        $(
            impl ToArg<'static> for $t {
                fn to_arg(self) -> Arg<'static> {
                    Arg::UInt(self as u64, width_of::<$t>())
                }
            }
        )*
    };
}
impl_to_arg_u!(u8, u16, u32, u64, usize);

/// List of printf arguments.
#[derive(Debug, Clone)]
pub struct ArgList<'a> {
    args: &'a [Arg<'a>],
    index: usize,
}

impl<'a> ArgList<'a> {
    /// Constuct a new arglist.
    pub fn new(args: &'a [Arg]) -> Self {
        Self { args, index: 0 }
    }

    /// Return how many args are remaining.
    pub fn remaining(&self) -> usize {
        self.args.len() - self.index
    }

    fn next_arg(&mut self) -> &Arg {
        let arg = &self.args[self.index];
        self.index += 1;
        arg
    }

    pub fn arg_i64(&mut self) -> i64 {
        let index = self.index;
        match self.next_arg() {
            Arg::Int(i, _) => *i,
            Arg::UInt(u, _) => *u as i64,
            x => panic!("expected {} at index {}, got {:?}", "int", index, x),
        }
    }

    pub fn arg_u64(&mut self) -> u64 {
        let index = self.index;
        match self.next_arg() {
            Arg::Int(i, _) => *i as u64,
            Arg::UInt(u, _) => *u,
            x => panic!("expected {} at index {}, got {:?}", "int", index, x),
        }
    }

    pub fn arg_i32(&mut self) -> i32 {
        self.arg_i64() as i32
    }

    pub fn arg_i16(&mut self) -> i16 {
        self.arg_i64() as i16
    }

    pub fn arg_i8(&mut self) -> i8 {
        self.arg_i64() as i8
    }

    pub fn arg_u32(&mut self) -> u32 {
        self.arg_u64() as u32
    }

    pub fn arg_u16(&mut self) -> u16 {
        self.arg_u64() as u16
    }

    pub fn arg_u8(&mut self) -> u8 {
        self.arg_u64() as u8
    }

    pub fn arg_f64(&mut self) -> f64 {
        let index = self.index;
        match self.next_arg() {
            Arg::Float(f) => *f,
            x => panic!("expected {} at index {}, got {:?}", "float", index, x),
        }
    }

    pub fn arg_c(&mut self) -> char {
        let index = self.index;
        match self.next_arg() {
            Arg::Char(c) => *c,
            x => panic!("expected {} at index {}, got {:?}", "char", index, x),
        }
    }

    pub fn arg_str(&mut self) -> &wstr {
        let index = self.index;
        match self.next_arg() {
            Arg::Str(s) => s,
            Arg::BoxedStr(s) => &*s,
            x => panic!("expected {} at index {}, got {:?}", "str", index, x),
        }
    }

    // Pointers are stored as integers.
    pub fn arg_p(&mut self) -> *const () {
        let index = self.index;
        match self.next_arg() {
            Arg::Int(i, _) => *i as *const (),
            Arg::UInt(u, _) => *u as *const (),
            x => panic!("expected {} at index {}, got {:?}", "int", index, x),
        }
    }
}
