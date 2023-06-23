use crate::ffi_tests::add_test;

add_test! {"test_string", || {
    use crate::wchar::WString;
    use crate::ffi::parser_t;
    use crate::ffi;
    use crate::builtins::string::string;
    use crate::wchar_ffi::WCharFromFFI;
    use crate::common::{EscapeStringStyle, escape_string};
    use crate::wchar::L;
    use crate::builtins::shared::{STATUS_CMD_ERROR,STATUS_CMD_OK, STATUS_INVALID_ARGS};
    use crate::future_feature_flags::{feature_test, FeatureFlag};
    use crate::future_feature_flags::mutable_fish_features;

    // TODO: these should be individual tests, not all in one, port when we can run these with `cargo test`
    macro_rules! string_test {
        ($args:expr, $expected_rc:expr, $expected_out:expr) => {
            let parser: &mut parser_t = unsafe { &mut *parser_t::principal_parser_ffi() };

            let mut streams = ffi::make_test_io_streams_ffi();
            let mut io = crate::builtins::shared::io_streams_t::new(streams.pin_mut());

            let rc = string(parser, &mut io, $args.as_mut_slice()).expect("string failed");

            assert_eq!(rc, $expected_rc.unwrap(), "string builtin returned unexpected return code");

            let string_stream = ffi::get_test_output_ffi(streams);
            let actual = escape_string(&string_stream.contents().from_ffi(), EscapeStringStyle::default());
            let expected = escape_string($expected_out, EscapeStringStyle::default());
            assert_eq!(expected, actual, "string builtin returned unexpected output");
        };
    }
    string_test!([L!("string"), L!("escape")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("escape"), L!("")], STATUS_CMD_OK, L!("''\n"));
    string_test!([L!("string"), L!("escape"), L!("-n"), L!("")], STATUS_CMD_OK, L!("\n"));
    string_test!([L!("string"), L!("escape"), L!("a")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("escape"), L!("\x07")], STATUS_CMD_OK, L!("\\cg\n"));
    string_test!([L!("string"), L!("escape"), L!("\"x\"")], STATUS_CMD_OK, L!("'\"x\"'\n"));
    string_test!([L!("string"), L!("escape"), L!("hello world")], STATUS_CMD_OK, L!("'hello world'\n"));
    string_test!([L!("string"), L!("escape"), L!("-n"), L!("hello world")], STATUS_CMD_OK, L!("hello\\ world\n"));
    string_test!([L!("string"), L!("escape"), L!("hello"), L!("world")], STATUS_CMD_OK, L!("hello\nworld\n"));
    string_test!([L!("string"), L!("escape"), L!("-n"), L!("~")], STATUS_CMD_OK, L!("\\~\n"));

    string_test!([L!("string"), L!("join")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("join"), L!("")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("join"), L!(""), L!(""), L!(""), L!("")], STATUS_CMD_OK, L!("\n"));
    string_test!([L!("string"), L!("join"), L!(""), L!("a"), L!("b"), L!("c")], STATUS_CMD_OK, L!("abc\n"));
    string_test!([L!("string"), L!("join"), L!("."), L!("fishshell"), L!("com")], STATUS_CMD_OK, L!("fishshell.com\n"));
    string_test!([L!("string"), L!("join"), L!("/"), L!("usr")], STATUS_CMD_ERROR, L!("usr\n"));
    string_test!([L!("string"), L!("join"), L!("/"), L!("usr"), L!("local"), L!("bin")], STATUS_CMD_OK, L!("usr/local/bin\n"));
    string_test!([L!("string"), L!("join"), L!("..."), L!("3"), L!("2"), L!("1")], STATUS_CMD_OK, L!("3...2...1\n"));
    string_test!([L!("string"), L!("join"), L!("-q")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("join"), L!("-q"), L!(".")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("join"), L!("-q"), L!("."), L!(".")], STATUS_CMD_ERROR, L!(""));

    string_test!([L!("string"), L!("length")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("length"), L!("")], STATUS_CMD_ERROR, L!("0\n"));
    string_test!([L!("string"), L!("length"), L!(""), L!(""), L!("")], STATUS_CMD_ERROR, L!("0\n0\n0\n"));
    string_test!([L!("string"), L!("length"), L!("a")], STATUS_CMD_OK, L!("1\n"));

// #if WCHAR_T_BITS > 16
    let wide_str = WString::from_chars([char::from_u32(0x2008A).unwrap()]);
    string_test!([L!("string"), L!("length"), &wide_str], STATUS_CMD_OK, L!("1\n"));
// #endif
    string_test!([L!("string"), L!("length"), L!("um"), L!("dois"), L!("três")], STATUS_CMD_OK, L!("2\n4\n4\n"));
    string_test!([L!("string"), L!("length"), L!("um"), L!("dois"), L!("três")], STATUS_CMD_OK, L!("2\n4\n4\n"));
    string_test!([L!("string"), L!("length"), L!("-q")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("length"), L!("-q"), L!("")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("length"), L!("-q"), L!("a")], STATUS_CMD_OK, L!(""));

    string_test!([L!("string"), L!("match")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("match"), L!("")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!(""), L!("")], STATUS_CMD_OK, L!("\n"));
    string_test!([L!("string"), L!("match"), L!("?"), L!("a")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("match"), L!("*"), L!("")], STATUS_CMD_OK, L!("\n"));
    string_test!([L!("string"), L!("match"), L!("**"), L!("")], STATUS_CMD_OK, L!("\n"));
    string_test!([L!("string"), L!("match"), L!("*"), L!("xyzzy")], STATUS_CMD_OK, L!("xyzzy\n"));
    string_test!([L!("string"), L!("match"), L!("**"), L!("plugh")], STATUS_CMD_OK, L!("plugh\n"));
    string_test!([L!("string"), L!("match"), L!("a*b"), L!("axxb")], STATUS_CMD_OK, L!("axxb\n"));
    string_test!([L!("string"), L!("match"), L!("a??b"), L!("axxb")], STATUS_CMD_OK, L!("axxb\n"));
    string_test!([L!("string"), L!("match"), L!("-i"), L!("a??B"), L!("axxb")], STATUS_CMD_OK, L!("axxb\n"));
    string_test!([L!("string"), L!("match"), L!("-i"), L!("a??b"), L!("Axxb")], STATUS_CMD_OK, L!("Axxb\n"));
    string_test!([L!("string"), L!("match"), L!("a*"), L!("axxb")], STATUS_CMD_OK, L!("axxb\n"));
    string_test!([L!("string"), L!("match"), L!("*a"), L!("xxa")], STATUS_CMD_OK, L!("xxa\n"));
    string_test!([L!("string"), L!("match"), L!("*a*"), L!("axa")], STATUS_CMD_OK, L!("axa\n"));
    string_test!([L!("string"), L!("match"), L!("*a*"), L!("xax")], STATUS_CMD_OK, L!("xax\n"));
    string_test!([L!("string"), L!("match"), L!("*a*"), L!("bxa")], STATUS_CMD_OK, L!("bxa\n"));
    string_test!([L!("string"), L!("match"), L!("*a"), L!("a")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("match"), L!("a*"), L!("a")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("match"), L!("a*b*c"), L!("axxbyyc")], STATUS_CMD_OK, L!("axxbyyc\n"));
    string_test!([L!("string"), L!("match"), L!("\\*"), L!("*")], STATUS_CMD_OK, L!("*\n"));
    string_test!([L!("string"), L!("match"), L!("a*\\"), L!("abc\\")], STATUS_CMD_OK, L!("abc\\\n"));
    string_test!([L!("string"), L!("match"), L!("a*\\?"), L!("abc?")], STATUS_CMD_OK, L!("abc?\n"));

    string_test!([L!("string"), L!("match"), L!("?"), L!("")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("?"), L!("ab")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("??"), L!("a")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("?a"), L!("a")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("a?"), L!("a")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("a??B"), L!("axxb")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("a*b"), L!("axxbc")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("*b"), L!("bbba")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("0x[0-9a-fA-F][0-9a-fA-F]"), L!("0xbad")], STATUS_CMD_ERROR, L!(""));

    string_test!([L!("string"), L!("match"), L!("-a"), L!("*"), L!("ab"), L!("cde")], STATUS_CMD_OK, L!("ab\ncde\n"));
    string_test!([L!("string"), L!("match"), L!("*"), L!("ab"), L!("cde")], STATUS_CMD_OK, L!("ab\ncde\n"));
    string_test!([L!("string"), L!("match"), L!("-n"), L!("*d*"), L!("cde")], STATUS_CMD_OK, L!("1 3\n"));
    string_test!([L!("string"), L!("match"), L!("-n"), L!("*x*"), L!("cde")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("-q"), L!("a*"), L!("b"), L!("c")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("-q"), L!("a*"), L!("b"), L!("a")], STATUS_CMD_OK, L!(""));

    string_test!([L!("string"), L!("match"), L!("-r")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("-r"), L!(""), L!("")], STATUS_CMD_OK, L!("\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("."), L!("a")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!(".*"), L!("")], STATUS_CMD_OK, L!("\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("a*b"), L!("b")], STATUS_CMD_OK, L!("b\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("a*b"), L!("aab")], STATUS_CMD_OK, L!("aab\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("-i"), L!("a*b"), L!("Aab")], STATUS_CMD_OK, L!("Aab\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("-a"), L!("a[bc]"), L!("abadac")], STATUS_CMD_OK, L!("ab\nac\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("a"), L!("xaxa"), L!("axax")], STATUS_CMD_OK, L!("a\na\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("-a"), L!("a"), L!("xaxa"), L!("axax")], STATUS_CMD_OK, L!("a\na\na\na\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("a[bc]"), L!("abadac")], STATUS_CMD_OK, L!("ab\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("-q"), L!("a[bc]"), L!("abadac")], STATUS_CMD_OK, L!(""));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("-q"), L!("a[bc]"), L!("ad")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("(a+)b(c)"), L!("aabc")],
     STATUS_CMD_OK,
     L!("aabc\naa\nc\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("-a"), L!("(a)b(c)"), L!("abcabc")],
     STATUS_CMD_OK,
     L!("abc\na\nc\nabc\na\nc\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("(a)b(c)"), L!("abcabc")],
     STATUS_CMD_OK,
     L!("abc\na\nc\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("(a|(z))(bc)"), L!("abc")],
     STATUS_CMD_OK,
     L!("abc\na\nbc\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("-n"), L!("a"), L!("ada"), L!("dad")],
     STATUS_CMD_OK,
     L!("1 1\n2 1\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("-n"), L!("-a"), L!("a"), L!("bacadae")],
     STATUS_CMD_OK,
     L!("2 1\n4 1\n6 1\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("-n"), L!("(a).*(b)"), L!("a---b")],
     STATUS_CMD_OK,
     L!("1 5\n1 1\n5 1\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("-n"), L!("(a)(b)"), L!("ab")],
     STATUS_CMD_OK,
     L!("1 2\n1 1\n2 1\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("-n"), L!("(a)(b)"), L!("abab")],
     STATUS_CMD_OK,
     L!("1 2\n1 1\n2 1\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("-n"), L!("-a"), L!("(a)(b)"), L!("abab")],
     STATUS_CMD_OK,
     L!("1 2\n1 1\n2 1\n3 2\n3 1\n4 1\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("*"), L!("")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("-a"), L!("a*"), L!("b")], STATUS_CMD_OK, L!("\n\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("foo\\Kbar"), L!("foobar")], STATUS_CMD_OK, L!("bar\n"));
    string_test!([L!("string"), L!("match"), L!("-r"), L!("(foo)\\Kbar"), L!("foobar")],
     STATUS_CMD_OK,
     L!("bar\nfoo\n"));
    string_test!([L!("string"), L!("replace")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("replace"), L!("")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("replace"), L!(""), L!("")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("replace"), L!(""), L!(""), L!("")], STATUS_CMD_ERROR, L!("\n"));
    string_test!([L!("string"), L!("replace"), L!(""), L!(""), L!(" ")], STATUS_CMD_ERROR, L!(" \n"));
    string_test!([L!("string"), L!("replace"), L!("a"), L!("b"), L!("")], STATUS_CMD_ERROR, L!("\n"));
    string_test!([L!("string"), L!("replace"), L!("a"), L!("b"), L!("a")], STATUS_CMD_OK, L!("b\n"));
    string_test!([L!("string"), L!("replace"), L!("a"), L!("b"), L!("xax")], STATUS_CMD_OK, L!("xbx\n"));
    string_test!([L!("string"), L!("replace"), L!("a"), L!("b"), L!("xax"), L!("axa")],
     STATUS_CMD_OK,
     L!("xbx\nbxa\n"));
    string_test!([L!("string"), L!("replace"), L!("bar"), L!("x"), L!("red barn")], STATUS_CMD_OK, L!("red xn\n"));
    string_test!([L!("string"), L!("replace"), L!("x"), L!("bar"), L!("red xn")], STATUS_CMD_OK, L!("red barn\n"));
    string_test!([L!("string"), L!("replace"), L!("--"), L!("x"), L!("-"), L!("xyz")], STATUS_CMD_OK, L!("-yz\n"));
    string_test!([L!("string"), L!("replace"), L!("--"), L!("y"), L!("-"), L!("xyz")], STATUS_CMD_OK, L!("x-z\n"));
    string_test!([L!("string"), L!("replace"), L!("--"), L!("z"), L!("-"), L!("xyz")], STATUS_CMD_OK, L!("xy-\n"));
    string_test!([L!("string"), L!("replace"), L!("-i"), L!("z"), L!("X"), L!("_Z_")], STATUS_CMD_OK, L!("_X_\n"));
    string_test!([L!("string"), L!("replace"), L!("-a"), L!("a"), L!("A"), L!("aaa")], STATUS_CMD_OK, L!("AAA\n"));
    string_test!([L!("string"), L!("replace"), L!("-i"), L!("a"), L!("z"), L!("AAA")], STATUS_CMD_OK, L!("zAA\n"));
    string_test!([L!("string"), L!("replace"), L!("-q"), L!("x"), L!(">x<"), L!("x")], STATUS_CMD_OK, L!(""));
    string_test!([L!("string"), L!("replace"), L!("-a"), L!("x"), L!(""), L!("xxx")], STATUS_CMD_OK, L!("\n"));
    string_test!([L!("string"), L!("replace"), L!("-a"), L!("***"), L!("_"), L!("*****")], STATUS_CMD_OK, L!("_**\n"));
    string_test!([L!("string"), L!("replace"), L!("-a"), L!("***"), L!("***"), L!("******")], STATUS_CMD_OK, L!("******\n"));
    string_test!([L!("string"), L!("replace"), L!("-a"), L!("a"), L!("b"), L!("xax"), L!("axa")], STATUS_CMD_OK, L!("xbx\nbxb\n"));

    string_test!([L!("string"), L!("replace"), L!("-r")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!(""), L!("")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!(""), L!(""), L!("")], STATUS_CMD_OK, L!("\n"));  // pcre2 behavior
    string_test!([L!("string"), L!("replace"), L!("-r"), L!(""), L!(""), L!(" ")], STATUS_CMD_OK, L!(" \n"));  // pcre2 behavior
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("a"), L!("b"), L!("")], STATUS_CMD_ERROR, L!("\n"));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("a"), L!("b"), L!("a")], STATUS_CMD_OK, L!("b\n"));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("."), L!("x"), L!("abc")], STATUS_CMD_OK, L!("xbc\n"));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("."), L!(""), L!("abc")], STATUS_CMD_OK, L!("bc\n"));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("(\\w)(\\w)"), L!("$2$1"), L!("ab")], STATUS_CMD_OK, L!("ba\n"));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("(\\w)"), L!("$1$1"), L!("ab")], STATUS_CMD_OK, L!("aab\n"));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("-a"), L!("."), L!("x"), L!("abc")], STATUS_CMD_OK, L!("xxx\n"));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("-a"), L!("(\\w)"), L!("$1$1"), L!("ab")], STATUS_CMD_OK, L!("aabb\n"));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("-a"), L!("."), L!(""), L!("abc")], STATUS_CMD_OK, L!("\n"));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("a"), L!("x"), L!("bc"), L!("cd"), L!("de")], STATUS_CMD_ERROR, L!("bc\ncd\nde\n"));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("a"), L!("x"), L!("aba"), L!("caa")], STATUS_CMD_OK, L!("xba\ncxa\n"));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("-a"), L!("a"), L!("x"), L!("aba"), L!("caa")], STATUS_CMD_OK, L!("xbx\ncxx\n"));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("-i"), L!("A"), L!("b"), L!("xax")], STATUS_CMD_OK, L!("xbx\n"));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("-i"), L!("[a-z]"), L!("."), L!("1A2B")], STATUS_CMD_OK, L!("1.2B\n"));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("A"), L!("b"), L!("xax")], STATUS_CMD_ERROR, L!("xax\n"));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("a"), L!("$1"), L!("a")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("(a)"), L!("$2"), L!("a")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("*"), L!("."), L!("a")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("replace"), L!("-ra"), L!("x"), L!("\\c")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("replace"), L!("-r"), L!("^(.)"), L!("\t$1"), L!("abc"), L!("x")], STATUS_CMD_OK, L!("\tabc\n\tx\n"));

    string_test!([L!("string"), L!("split")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("split"), L!(":")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("split"), L!("."), L!("www.ch.ic.ac.uk")], STATUS_CMD_OK, L!("www\nch\nic\nac\nuk\n"));
    string_test!([L!("string"), L!("split"), L!(".."), L!("....")], STATUS_CMD_OK, L!("\n\n\n"));
    string_test!([L!("string"), L!("split"), L!("-m"), L!("x"), L!(".."), L!("....")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("split"), L!("-m1"), L!(".."), L!("....")], STATUS_CMD_OK, L!("\n..\n"));
    string_test!([L!("string"), L!("split"), L!("-m0"), L!("/"), L!("/usr/local/bin/fish")], STATUS_CMD_ERROR, L!("/usr/local/bin/fish\n"));
    string_test!([L!("string"), L!("split"), L!("-m2"), L!(":"), L!("a:b:c:d"), L!("e:f:g:h")], STATUS_CMD_OK, L!("a\nb\nc:d\ne\nf\ng:h\n"));
    string_test!([L!("string"), L!("split"), L!("-m1"), L!("-r"), L!("/"), L!("/usr/local/bin/fish")], STATUS_CMD_OK, L!("/usr/local/bin\nfish\n"));
    string_test!([L!("string"), L!("split"), L!("-r"), L!("."), L!("www.ch.ic.ac.uk")], STATUS_CMD_OK, L!("www\nch\nic\nac\nuk\n"));
    string_test!([L!("string"), L!("split"), L!("--"), L!("--"), L!("a--b---c----d")], STATUS_CMD_OK, L!("a\nb\n-c\n\nd\n"));
    string_test!([L!("string"), L!("split"), L!("-r"), L!(".."), L!("....")], STATUS_CMD_OK, L!("\n\n\n"));
    string_test!([L!("string"), L!("split"), L!("-r"), L!("--"), L!("--"), L!("a--b---c----d")], STATUS_CMD_OK, L!("a\nb-\nc\n\nd\n"));
    string_test!([L!("string"), L!("split"), L!(""), L!("")], STATUS_CMD_ERROR, L!("\n"));
    string_test!([L!("string"), L!("split"), L!(""), L!("a")], STATUS_CMD_ERROR, L!("a\n"));
    string_test!([L!("string"), L!("split"), L!(""), L!("ab")], STATUS_CMD_OK, L!("a\nb\n"));
    string_test!([L!("string"), L!("split"), L!(""), L!("abc")], STATUS_CMD_OK, L!("a\nb\nc\n"));
    string_test!([L!("string"), L!("split"), L!("-m1"), L!(""), L!("abc")], STATUS_CMD_OK, L!("a\nbc\n"));
    string_test!([L!("string"), L!("split"), L!("-r"), L!(""), L!("")], STATUS_CMD_ERROR, L!("\n"));
    string_test!([L!("string"), L!("split"), L!("-r"), L!(""), L!("a")], STATUS_CMD_ERROR, L!("a\n"));
    string_test!([L!("string"), L!("split"), L!("-r"), L!(""), L!("ab")], STATUS_CMD_OK, L!("a\nb\n"));
    string_test!([L!("string"), L!("split"), L!("-r"), L!(""), L!("abc")], STATUS_CMD_OK, L!("a\nb\nc\n"));
    string_test!([L!("string"), L!("split"), L!("-r"), L!("-m1"), L!(""), L!("abc")], STATUS_CMD_OK, L!("ab\nc\n"));
    string_test!([L!("string"), L!("split"), L!("-q")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("split"), L!("-q"), L!(":")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("split"), L!("-q"), L!("x"), L!("axbxc")], STATUS_CMD_OK, L!(""));

    string_test!([L!("string"), L!("sub")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("sub"), L!("abcde")], STATUS_CMD_OK, L!("abcde\n"));
    string_test!([L!("string"), L!("sub"), L!("-L!("), L!(")x"), L!("abcde")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("sub"), L!("-s"), L!("x"), L!("abcde")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("sub"), L!("-l0"), L!("abcde")], STATUS_CMD_OK, L!("\n"));
    string_test!([L!("string"), L!("sub"), L!("-l2"), L!("abcde")], STATUS_CMD_OK, L!("ab\n"));
    string_test!([L!("string"), L!("sub"), L!("-l5"), L!("abcde")], STATUS_CMD_OK, L!("abcde\n"));
    string_test!([L!("string"), L!("sub"), L!("-l6"), L!("abcde")], STATUS_CMD_OK, L!("abcde\n"));
    string_test!([L!("string"), L!("sub"), L!("-l-1"), L!("abcde")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("sub"), L!("-s0"), L!("abcde")], STATUS_INVALID_ARGS, L!(""));
    string_test!([L!("string"), L!("sub"), L!("-s1"), L!("abcde")], STATUS_CMD_OK, L!("abcde\n"));
    string_test!([L!("string"), L!("sub"), L!("-s5"), L!("abcde")], STATUS_CMD_OK, L!("e\n"));
    string_test!([L!("string"), L!("sub"), L!("-s6"), L!("abcde")], STATUS_CMD_OK, L!("\n"));
    string_test!([L!("string"), L!("sub"), L!("-s-1"), L!("abcde")], STATUS_CMD_OK, L!("e\n"));
    string_test!([L!("string"), L!("sub"), L!("-s-5"), L!("abcde")], STATUS_CMD_OK, L!("abcde\n"));
    string_test!([L!("string"), L!("sub"), L!("-s-6"), L!("abcde")], STATUS_CMD_OK, L!("abcde\n"));
    string_test!([L!("string"), L!("sub"), L!("-s1"), L!("-l0"), L!("abcde")], STATUS_CMD_OK, L!("\n"));
    string_test!([L!("string"), L!("sub"), L!("-s1"), L!("-l1"), L!("abcde")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("sub"), L!("-s2"), L!("-l2"), L!("abcde")], STATUS_CMD_OK, L!("bc\n"));
    string_test!([L!("string"), L!("sub"), L!("-s-1"), L!("-l1"), L!("abcde")], STATUS_CMD_OK, L!("e\n"));
    string_test!([L!("string"), L!("sub"), L!("-s-1"), L!("-l2"), L!("abcde")], STATUS_CMD_OK, L!("e\n"));
    string_test!([L!("string"), L!("sub"), L!("-s-3"), L!("-l2"), L!("abcde")], STATUS_CMD_OK, L!("cd\n"));
    string_test!([L!("string"), L!("sub"), L!("-s-3"), L!("-l4"), L!("abcde")], STATUS_CMD_OK, L!("cde\n"));
    string_test!([L!("string"), L!("sub"), L!("-q")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("sub"), L!("-q"), L!("abcde")], STATUS_CMD_OK, L!(""));

    string_test!([L!("string"), L!("trim")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("trim"), L!("")], STATUS_CMD_ERROR, L!("\n"));
    string_test!([L!("string"), L!("trim"), L!(" ")], STATUS_CMD_OK, L!("\n"));
    string_test!([L!("string"), L!("trim"), L!("  \x0C\n\r\t")], STATUS_CMD_OK, L!("\n"));
    string_test!([L!("string"), L!("trim"), L!(" a")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("trim"), L!("a ")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("trim"), L!(" a ")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("trim"), L!("-L!("), L!(") a")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("trim"), L!("-L!("), L!(")a ")], STATUS_CMD_ERROR, L!("a \n"));
    string_test!([L!("string"), L!("trim"), L!("-L!("), L!(") a ")], STATUS_CMD_OK, L!("a \n"));
    string_test!([L!("string"), L!("trim"), L!("-r"), L!(" a")], STATUS_CMD_ERROR, L!(" a\n"));
    string_test!([L!("string"), L!("trim"), L!("-r"), L!("a ")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("trim"), L!("-r"), L!(" a ")], STATUS_CMD_OK, L!(" a\n"));
    string_test!([L!("string"), L!("trim"), L!("-c"), L!("."), L!(" a")], STATUS_CMD_ERROR, L!(" a\n"));
    string_test!([L!("string"), L!("trim"), L!("-c"), L!("."), L!("a ")], STATUS_CMD_ERROR, L!("a \n"));
    string_test!([L!("string"), L!("trim"), L!("-c"), L!("."), L!(" a ")], STATUS_CMD_ERROR, L!(" a \n"));
    string_test!([L!("string"), L!("trim"), L!("-c"), L!("."), L!(".a")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("trim"), L!("-c"), L!("."), L!("a.")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("trim"), L!("-c"), L!("."), L!(".a.")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("trim"), L!("-c"), L!("\\/"), L!("/a\\")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("trim"), L!("-c"), L!("\\/"), L!("a/")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("trim"), L!("-c"), L!("\\/"), L!("\\a/")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("trim"), L!("-c"), L!(""), L!(".a.")], STATUS_CMD_ERROR, L!(".a.\n"));

    let saved_flag = feature_test(FeatureFlag::qmark_noglob);
    unsafe { mutable_fish_features().as_mut() }.unwrap().set(FeatureFlag::qmark_noglob, true);
    string_test!([L!("string"), L!("match"), L!("a*b?c"), L!("axxb?c")], STATUS_CMD_OK, L!("axxb?c\n"));
    string_test!([L!("string"), L!("match"), L!("*?"), L!("a")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("*?"), L!("ab")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("?*"), L!("a")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("?*"), L!("ab")], STATUS_CMD_ERROR, L!(""));
    string_test!([L!("string"), L!("match"), L!("a*\\?"), L!("abc?")], STATUS_CMD_ERROR, L!(""));

    unsafe { mutable_fish_features().as_mut() }.unwrap().set(FeatureFlag::qmark_noglob, false);
    string_test!([L!("string"), L!("match"), L!("a*b?c"), L!("axxbyc")], STATUS_CMD_OK, L!("axxbyc\n"));
    string_test!([L!("string"), L!("match"), L!("*?"), L!("a")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("match"), L!("*?"), L!("ab")], STATUS_CMD_OK, L!("ab\n"));
    string_test!([L!("string"), L!("match"), L!("?*"), L!("a")], STATUS_CMD_OK, L!("a\n"));
    string_test!([L!("string"), L!("match"), L!("?*"), L!("ab")], STATUS_CMD_OK, L!("ab\n"));
    string_test!([L!("string"), L!("match"), L!("a*\\?"), L!("abc?")], STATUS_CMD_OK, L!("abc?\n"));

    unsafe { mutable_fish_features().as_mut() }.unwrap().set(FeatureFlag::qmark_noglob, saved_flag);

}}
