use pcre2::utf32::Captures;
use pcre2::utf32::{Regex, RegexBuilder};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::iter;
use std::ops::Deref;
use std::os::fd::FromRawFd;

use crate::builtins::shared::{
    builtin_missing_argument, builtin_print_help, io_streams_t, BUILTIN_ERR_ARG_COUNT0,
    BUILTIN_ERR_ARG_COUNT1, BUILTIN_ERR_NOT_NUMBER, BUILTIN_ERR_UNKNOWN, STATUS_CMD_ERROR,
    STATUS_CMD_OK, STATUS_INVALID_ARGS,
};
use crate::builtins::shared::{
    builtin_print_error_trailer, BUILTIN_ERR_COMBO2, BUILTIN_ERR_INVALID_SUBCMD,
    BUILTIN_ERR_MISSING_SUBCMD,
};
use crate::common::{escape_string, str2wcstring};
use crate::common::{get_ellipsis_str, EscapeFlags};
use crate::common::{unescape_string, EscapeStringStyle, UnescapeStringStyle};
use crate::env::{EnvMode, EnvVar, EnvVarFlags};
use crate::fallback::fish_wcwidth;
use crate::ffi::parser_t;
use crate::flog::FLOG;

use crate::future_feature_flags::{feature_test, FeatureFlag};
use crate::io::OutputStream;
use crate::io::SeparationType;
use crate::parse_util::parse_util_unescape_wildcards;
use crate::wchar::{wstr, WString, L};
use crate::wchar_ext::WExt;
use crate::wchar_ffi::WCharToFFI;
use crate::wcstringutil::{fish_wcwidth_visible, split_about, split_string};
use crate::wgetopt::{wgetopter_t, wopt, woption, woption_argument_t};
use crate::wildcard::ANY_STRING;
use crate::wutil::{fish_wcstol, fish_wcswidth, wgettext_fmt};
use libc::c_int;

use super::shared::BUILTIN_ERR_TOO_MANY_ARGUMENTS;

macro_rules! string_error {
    (
    $streams:expr,
    $string:expr
    $(, $args:expr)+
    $(,)?
    ) => {
        $streams.err.append(L!("string "));
        $streams.err.append(wgettext_fmt!($string, $($args),*));
    };
}

const STRING_CHUNK_SIZE: usize = 1024;

fn try_compile_regex(
    pattern: &wstr,
    ignore_case: bool,
    cmd: &wstr,
    streams: &mut io_streams_t,
) -> Option<Regex> {
    match RegexBuilder::new()
        .caseless(ignore_case)
        .build(pattern.as_char_slice())
    {
        Ok(r) => Some(r),
        Err(e) => {
            string_error!(
                streams,
                "%ls: Regular expression compile error: %ls\n",
                cmd,
                &WString::from(e.error_message())
            );
            string_error!(streams, "%ls: %ls\n", cmd, pattern);
            string_error!(streams, "%ls: %*ls\n", cmd, e.offset().unwrap(), "^");
            return None;
        }
    }
}

#[allow(clippy::type_complexity)]
const SUBCOMMANDS: &[(&wstr, fn() -> Box<dyn StringSubCommand>)] = &[
    (L!("collect"), || Box::<Collect>::default()),
    (L!("escape"), || Box::<Escape>::default()),
    (L!("join"), || Box::<Join>::default()),
    (L!("join0"), || {
        let mut cmd = Box::<Join>::default();
        cmd.is_join0 = true;
        cmd
    }),
    (L!("length"), || Box::<Length>::default()),
    (L!("lower"), || {
        let cmd = Transform {
            quiet: false,
            func: wstr::to_lowercase,
        };
        Box::new(cmd)
    }),
    (L!("match"), || Box::<Match>::default()),
    (L!("pad"), || Box::<Pad>::default()),
    (L!("repeat"), || Box::<Repeat>::default()),
    (L!("replace"), || Box::<Replace>::default()),
    (L!("shorten"), || Box::<Shorten>::default()),
    (L!("split"), || Box::<Split>::default()),
    (L!("split0"), || {
        Box::new(Split {
            is_split0: true,
            ..Default::default()
        })
    }),
    (L!("sub"), || Box::<Sub>::default()),
    (L!("trim"), || Box::<Trim>::default()),
    (L!("unescape"), || Box::<Unescape>::default()),
    (L!("upper"), || {
        let cmd = Transform {
            quiet: false,
            func: wstr::to_uppercase,
        };
        Box::new(cmd)
    }),
];
assert_sorted_by_name!(SUBCOMMANDS, 0);

fn string_unknown_option(
    parser: &mut parser_t,
    streams: &mut io_streams_t,
    subcmd: &wstr,
    opt: &wstr,
) {
    string_error!(streams, BUILTIN_ERR_UNKNOWN, subcmd, opt);
    builtin_print_error_trailer(parser, streams, L!("string"));
}

trait SubCmdOptions {
    // most of what is below is a (as minimally convoluted) way of making StringSubCommand object safe
    const SHORT_OPTIONS: &'static wstr;
    const LONG_OPTIONS: &'static [woption<'static>];
}

trait SubCmdHandler {
    fn parse_options(&mut self, optarg: Option<&wstr>, c: char) -> Result<(), ParseError>;

    fn handle(
        &mut self,
        parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int>;

    #[allow(unused_variables)]
    fn take_args(
        &mut self,
        optind: &mut usize,
        args: &[&wstr],
        streams: &mut io_streams_t,
    ) -> Option<c_int> {
        STATUS_CMD_OK
    }
}

trait StringSubCommand {
    // has to be funcs instead of associated consts to be object safe
    // having it as two traits with blanket impls works though
    fn short_options(&self) -> &'static wstr;
    fn long_options(&self) -> &'static [woption<'static>];
    fn parse_options(&mut self, optarg: Option<&wstr>, c: char) -> Result<(), ParseError>;
    fn take_args(
        &mut self,
        optind: &mut usize,
        args: &[&wstr],
        streams: &mut io_streams_t,
    ) -> Option<c_int>;
    fn handle(
        &mut self,
        parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int>;
}

impl<T> StringSubCommand for T
where
    T: SubCmdOptions + SubCmdHandler,
{
    fn short_options(&self) -> &'static wstr {
        Self::SHORT_OPTIONS
    }

    fn long_options(&self) -> &'static [woption<'static>] {
        Self::LONG_OPTIONS
    }

    fn parse_options(&mut self, optarg: Option<&wstr>, c: char) -> Result<(), ParseError> {
        self.parse_options(optarg, c)
    }

    fn take_args(
        &mut self,
        optind: &mut usize,
        args: &[&wstr],
        streams: &mut io_streams_t,
    ) -> Option<c_int> {
        self.take_args(optind, args, streams)
    }

    fn handle(
        &mut self,
        parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int> {
        self.handle(parser, streams, optind, args)
    }
}

fn parse_opts(
    subcmd: &mut Box<dyn StringSubCommand>,
    optind: &mut usize,
    args: &mut [&wstr],
    parser: &mut parser_t,
    streams: &mut io_streams_t,
) -> Option<c_int> {
    let cmd = args[0];
    let mut args_read = Vec::with_capacity(args.len());
    args_read.extend_from_slice(args);

    let mut w = wgetopter_t::new(subcmd.short_options(), subcmd.long_options(), args);
    while let Some(c) = w.wgetopt_long() {
        match c {
            ':' => {
                streams.err.append(L!("string ")); // clone of string_error
                builtin_missing_argument(parser, streams, cmd, args_read[w.woptind - 1], false);
                return STATUS_INVALID_ARGS;
            }
            '?' => {
                string_unknown_option(parser, streams, cmd, args_read[w.woptind - 1]);
                return STATUS_INVALID_ARGS;
            }
            c => {
                let retval = subcmd.parse_options(w.woptarg, c);
                if let Err(e) = retval {
                    e.print_error(&mut args_read, parser, streams, w.woptarg, w.woptind);
                    return e.retval();
                }
            }
        }
    }

    // TODO: does not take args into account
    *optind = w.woptind;

    return STATUS_CMD_OK;
}

fn width_without_escapes(ins: &wstr, start_pos: usize) -> i32 {
    let mut width = 0i32;
    for c in ins[start_pos..].chars() {
        let w = fish_wcwidth_visible(c);
        // We assume that this string is on its own line,
        // in which case a backslash can't bring us below 0.
        if w > 0 || width > 0 {
            width += w;
        }
    }
    // ANSI escape sequences like \e\[31m contain printable characters. Subtract their width
    // because they are not rendered.
    let mut pos = start_pos;
    while let Some(ec_pos) = ins.slice_from(pos).find_char('\x1B') {
        if let Some(len) = escape_code_length(ins.slice_from(ec_pos)) {
            let sub = ins.slice_from(ec_pos).slice_to(len);
            for c in sub.chars() {
                width -= fish_wcwidth_visible(c);
            }
            // Move us forward behind the escape code,
            // it might include a second escape!
            // E.g. SGR0 ("reset") is \e\(B\e\[m in xterm.
            pos = ec_pos + len - 1;
        } else {
            pos = ec_pos + 1;
        }
    }

    return width;
}

fn escape_code_length(code: &wstr) -> Option<usize> {
    use crate::ffi::escape_code_length_ffi;
    match escape_code_length_ffi(code.as_ptr()).0 {
        -1 => None,
        n => Some(n as usize),
    }
}

enum ParseError {
    InvalidArgs(&'static str),
    NotANumber,
    UnknownOption,
}

impl ParseError {
    fn description(&self) -> &str {
        match self {
            ParseError::InvalidArgs(_) => "Invalid arguments",
            ParseError::NotANumber => "Not a number",
            ParseError::UnknownOption => "Unknown option",
        }
    }
}

impl ParseError {
    fn print_error(
        &self,
        args: &mut [&wstr],
        parser: &mut parser_t,
        streams: &mut io_streams_t,
        optarg: Option<&wstr>,
        optind: usize,
    ) {
        match self {
            ParseError::InvalidArgs(s) => {
                let error_msg =
                    wgettext_fmt!("%ls: Invalid %ls '%ls'\n", args[0], s, optarg.unwrap());
                // TODO: might be +1 from unknown opt's thingy, is actually optarg

                streams.err.append(L!("string "));
                streams.err.append(error_msg);
            }
            ParseError::NotANumber => {
                string_error!(streams, BUILTIN_ERR_NOT_NUMBER, args[0], optarg.unwrap());
            }
            ParseError::UnknownOption => {
                string_unknown_option(parser, streams, args[0], args[optind - 1]);
            }
        }
    }

    fn retval(&self) -> Option<c_int> {
        STATUS_INVALID_ARGS
    }
}

#[derive(Default)]
struct Collect {
    allow_empty: bool,
    no_trim_newlines: bool,
}

impl SubCmdOptions for Collect {
    const LONG_OPTIONS: &'static [woption<'static>] = &[
        wopt(L!("allow-empty"), woption_argument_t::no_argument, 'a'),
        wopt(L!("no-trim-newlines"), woption_argument_t::no_argument, 'N'),
    ];
    const SHORT_OPTIONS: &'static wstr = L!(":Na");
}

impl SubCmdHandler for Collect {
    fn parse_options(&mut self, _optarg: Option<&wstr>, c: char) -> Result<(), ParseError> {
        match c {
            'a' => self.allow_empty = true,
            'N' => self.no_trim_newlines = true,
            _ => return Err(ParseError::UnknownOption),
        }
        Ok(())
    }

    fn handle(
        &mut self,
        _parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int> {
        let mut appended = 0usize;
        let mut iter = Arguments::new(args, optind, false);
        while let Some(mut arg) = iter.next(streams) {
            if !self.no_trim_newlines {
                let trim_len = arg.len() - arg.chars().rev().take_while(|&c| c == '\n').count();
                arg.to_mut().truncate(trim_len);
            }

            streams.out.append_with_separation(
                &arg,
                SeparationType::explicitly,
                iter.want_newline(),
            );
            appended += arg.len();
        }

        // If we haven't printed anything and "no_empty" is set,
        // print something empty. Helps with empty ellision:
        // echo (true | string collect --allow-empty)"bar"
        // prints "bar".
        if self.allow_empty && appended == 0 {
            streams.out.append_with_separation(
                L!(""),
                SeparationType::explicitly,
                true, /* historical behavior is to always print a newline */
            );
        }

        if appended > 0 {
            STATUS_CMD_OK
        } else {
            STATUS_CMD_ERROR
        }
    }
}

#[derive(Default)]
struct Escape {
    no_quoted: bool,
    style: EscapeStringStyle,
}

impl SubCmdOptions for Escape {
    const LONG_OPTIONS: &'static [woption<'static>] = &[
        wopt(L!("no-quoted"), woption_argument_t::no_argument, 'n'),
        wopt(L!("style"), woption_argument_t::required_argument, '\u{1}'),
    ];
    const SHORT_OPTIONS: &'static wstr = L!(":n");
}

impl SubCmdHandler for Escape {
    fn parse_options(&mut self, optarg: Option<&wstr>, c: char) -> Result<(), ParseError> {
        match c {
            'n' => self.no_quoted = true,
            '\u{1}' => {
                let optarg = optarg.expect("option --style requires an argument");

                self.style = EscapeStringStyle::try_from(optarg)
                    .map_err(|_| ParseError::InvalidArgs("escape style"))?;
            }
            _ => return Err(ParseError::UnknownOption),
        }
        return Ok(());
    }

    fn handle(
        &mut self,
        _parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int> {
        // Currently, only the script style supports options.
        // Ignore them for other styles for now.
        let style = match self.style {
            EscapeStringStyle::Script(..) if self.no_quoted => {
                EscapeStringStyle::Script(EscapeFlags::NO_QUOTED)
            }
            x => x,
        };

        let mut escaped_any = false;
        let mut iter = Arguments::new(args, optind, true);
        while let Some(arg) = iter.next(streams) {
            let mut escaped = escape_string(&arg, style);

            if iter.want_newline() {
                escaped.push('\n');
            }

            streams.out.append(escaped);
            escaped_any = true;
        }

        if escaped_any {
            STATUS_CMD_OK
        } else {
            STATUS_CMD_ERROR
        }
    }
}

#[derive(Default)]
struct Join {
    quiet: bool,
    no_empty: bool,
    is_join0: bool,
    // we _could_ just take a reference, but the life-time parameters are a bit much
    sep: WString,
}

impl SubCmdOptions for Join {
    const LONG_OPTIONS: &'static [woption<'static>] = &[
        wopt(L!("quiet"), woption_argument_t::no_argument, 'q'),
        wopt(L!("no-empty"), woption_argument_t::no_argument, 'n'),
    ];
    const SHORT_OPTIONS: &'static wstr = L!(":qn");
}

impl SubCmdHandler for Join {
    fn take_args(
        &mut self,
        optind: &mut usize,
        args: &[&wstr],
        streams: &mut io_streams_t,
    ) -> Option<c_int> {
        if self.is_join0 {
            return STATUS_CMD_OK;
        }

        let Some(arg) = args.get(*optind).copied() else {
           string_error!(streams, BUILTIN_ERR_ARG_COUNT0, args[0]);
           return STATUS_INVALID_ARGS;
       };
        *optind += 1;
        self.sep = arg.to_owned();

        STATUS_CMD_OK
    }
    fn parse_options(&mut self, _optarg: Option<&wstr>, c: char) -> Result<(), ParseError> {
        match c {
            'q' => self.quiet = true,
            'n' => self.no_empty = true,
            _ => return Err(ParseError::UnknownOption),
        }
        return Ok(());
    }

    fn handle(
        &mut self,
        _parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int> {
        let sep = &self.sep;
        let mut nargs = 0usize;
        let mut iter = Arguments::new(args, optind, true);
        while let Some(arg) = iter.next(streams) {
            if !self.quiet {
                if self.no_empty && arg.is_empty() {
                    continue;
                }

                if nargs > 0 {
                    streams.out.append(sep);
                }

                streams.out.append(arg);
            } else if nargs > 1 {
                return STATUS_CMD_OK;
            }
            nargs += 1;
        }

        if nargs > 0 && !self.quiet {
            if self.is_join0 {
                streams.out.append1('\0');
            } else if iter.want_newline() {
                streams.out.append1('\n');
            }
        }

        if nargs > 1 {
            STATUS_CMD_OK
        } else {
            STATUS_CMD_ERROR
        }
    }
}

#[derive(Default)]
struct Length {
    quiet: bool,
    visible: bool,
}

impl SubCmdOptions for Length {
    const LONG_OPTIONS: &'static [woption<'static>] = &[
        wopt(L!("quiet"), woption_argument_t::no_argument, 'q'),
        wopt(L!("visible"), woption_argument_t::no_argument, 'V'),
    ];
    const SHORT_OPTIONS: &'static wstr = L!(":qV");
}

impl SubCmdHandler for Length {
    fn parse_options(&mut self, _optarg: Option<&wstr>, c: char) -> Result<(), ParseError> {
        match c {
            'q' => self.quiet = true,
            'V' => self.visible = true,
            _ => return Err(ParseError::UnknownOption),
        }
        return Ok(());
    }

    fn handle(
        &mut self,
        _parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int> {
        let mut nnonempty = 0usize;
        let mut iter = Arguments::new(args, optind, true);
        while let Some(arg) = iter.next(streams) {
            if self.visible {
                // Visible length only makes sense line-wise.
                for line in split_string(&arg, '\n') {
                    let mut max = 0;
                    // Carriage-return returns us to the beginning. The longest substring without
                    // carriage-return determines the overall width.
                    for reset in split_string(&line, '\r') {
                        let n = width_without_escapes(&reset, 0);
                        max = max.max(n);
                    }
                    if max > 0 {
                        nnonempty += 1;
                    }
                    if !self.quiet {
                        streams
                            .out
                            .append(WString::from(max.to_string()) + L!("\n"));
                    } else if nnonempty > 0 {
                        return STATUS_CMD_OK;
                    }
                }
            } else {
                let n = arg.len();
                if n > 0 {
                    nnonempty += 1;
                }
                if !self.quiet {
                    streams.out.append(WString::from(n.to_string()) + L!("\n"));
                } else if nnonempty > 0 {
                    return STATUS_CMD_OK;
                }
            }
        }
        if nnonempty > 0 {
            STATUS_CMD_OK
        } else {
            STATUS_CMD_ERROR
        }
    }
}

struct Transform {
    quiet: bool,
    func: fn(&wstr) -> WString,
}

impl SubCmdOptions for Transform {
    const LONG_OPTIONS: &'static [woption<'static>] =
        &[wopt(L!("quiet"), woption_argument_t::no_argument, 'q')];
    const SHORT_OPTIONS: &'static wstr = L!(":q");
}

impl SubCmdHandler for Transform {
    fn parse_options(&mut self, _optarg: Option<&wstr>, c: char) -> Result<(), ParseError> {
        match c {
            'q' => self.quiet = true,
            _ => return Err(ParseError::UnknownOption),
        }
        return Ok(());
    }

    fn handle(
        &mut self,
        _parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int> {
        let mut n_transformed = 0usize;
        let mut iter = Arguments::new(args, optind, true);
        while let Some(arg) = iter.next(streams) {
            let transformed = (self.func)(&arg);
            if transformed != arg {
                n_transformed += 1;
            }
            if !self.quiet {
                let sep = if iter.want_newline() { '\n' } else { '\0' };
                streams.out.append(&transformed);
                streams.out.append1(sep);
            } else if n_transformed > 0 {
                return STATUS_CMD_OK;
            }
        }

        if n_transformed > 0 {
            STATUS_CMD_OK
        } else {
            STATUS_CMD_ERROR
        }
    }
}

enum StringMatcher<'opts> {
    Regex {
        regex: Box<Regex>,
        total_matched: usize,
        first_match_captures: HashMap<String, Vec<WString>>,
        opts: &'opts Match,
    },
    WildCard {
        pattern: WString,
        total_matched: usize,
        opts: &'opts Match,
    },
}

enum MatchResult<'a> {
    NoMatch,
    Match(Option<Captures<'a>>),
}

fn report_match<'a>(
    arg: &'a wstr,
    matches: &mut impl Iterator<Item = Result<Captures<'a>, pcre2::Error>>,
    opts: &Match,
    streams: &mut io_streams_t,
) -> Result<MatchResult<'a>, pcre2::Error> {
    let cg = match matches.next() {
        // 0th capture group corresponds to entire match
        Some(Ok(cg)) if cg.get(0).is_some() => cg,
        Some(Err(e)) => return Err(e),
        _ => {
            if opts.invert_match && !opts.quiet {
                if opts.index {
                    streams.out.append(wgettext_fmt!("1 %lu\n", arg.len()));
                } else {
                    streams.out.append(arg);
                    streams.out.append1('\n');
                }
            }
            return Ok(match opts.invert_match {
                true => MatchResult::Match(None),
                false => MatchResult::NoMatch,
            });
        }
    };

    if opts.invert_match {
        return Ok(MatchResult::NoMatch);
    }

    if opts.quiet {
        return Ok(MatchResult::Match(Some(cg)));
    }

    if opts.entire {
        streams.out.append(arg);
        streams.out.append1('\n');
    }

    let start = (opts.entire || opts.groups_only) as usize;

    for m in (start..cg.len()).filter_map(|i| cg.get(i)) {
        if opts.index {
            streams.out.append(wgettext_fmt!(
                "%lu %lu\n",
                m.start() + 1,
                m.end() - m.start()
            ));
        } else {
            streams.out.append(&arg[m.start()..m.end()]);
            streams.out.append1('\n');
        }
    }

    return Ok(MatchResult::Match(Some(cg)));
}

fn populate_captures_from_match<'a>(
    opts: &'a Match,
    first_match_captures: &mut HashMap<String, Vec<WString>>,
    cg: &'a Option<Captures<'a>>,
) {
    for (name, captures) in first_match_captures.iter_mut() {
        // If there are multiple named groups and --all was used, we need to ensure that
        // the indexes are always in sync between the variables. If an optional named
        // group didn't match but its brethren did, we need to make sure to put
        // *something* in the resulting array, and unfortunately fish doesn't support
        // empty/null members so we're going to have to use an empty string as the
        // sentinel value.

        if let Some(m) = cg.as_ref().and_then(|cg| cg.name(&name.to_string())) {
            captures.push(WString::from(m.as_bytes()));
        } else if opts.all {
            captures.push(WString::new());
        }
    }
}

impl StringMatcher<'_> {
    fn report_matches(
        &mut self,
        arg: &wstr,
        streams: &mut io_streams_t,
    ) -> Result<(), pcre2::Error> {
        match self {
            StringMatcher::Regex {
                regex,
                total_matched,
                first_match_captures,
                opts,
            } => {
                let mut iter = regex.captures_iter(arg.as_char_slice());
                let rc = report_match(arg, &mut iter, opts, streams)?;

                let mut populate_captures = false;
                if let MatchResult::Match(actual) = &rc {
                    populate_captures = *total_matched == 0;
                    *total_matched += 1;

                    if populate_captures {
                        populate_captures_from_match(opts, first_match_captures, actual);
                    }
                }

                if !opts.invert_match && opts.all {
                    // we are guaranteed to match as long as ops.invert_match is false
                    while let MatchResult::Match(cg) = report_match(arg, &mut iter, opts, streams)?
                    {
                        if populate_captures {
                            populate_captures_from_match(opts, first_match_captures, &cg);
                        }
                    }
                }
            }
            StringMatcher::WildCard {
                pattern,
                total_matched,
                opts,
            } => {
                use crate::ffi::wildcard_match;
                let subject = match opts.ignore_case {
                    true => arg.to_lowercase(),
                    false => arg.to_owned(),
                };
                let m = wildcard_match(&subject.to_ffi(), &pattern.to_ffi(), false);

                if m ^ opts.invert_match {
                    *total_matched += 1;
                    if !opts.quiet {
                        if opts.index {
                            streams.out.append(wgettext_fmt!("1 %lu\n", arg.len()));
                        } else {
                            streams.out.append(arg);
                            streams.out.append1('\n');
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn match_count(&self) -> usize {
        match self {
            StringMatcher::Regex { total_matched, .. } => *total_matched,
            StringMatcher::WildCard { total_matched, .. } => *total_matched,
        }
    }
}

impl Match {
    fn validate_capture_group_names(
        capture_group_names: &[Option<String>],
        streams: &mut io_streams_t,
    ) -> bool {
        for name in capture_group_names.iter().filter_map(|n| n.as_ref()) {
            if EnvVar::flags_for(&WString::from_str(name)) == EnvVarFlags::READ_ONLY {
                streams.err.append(wgettext_fmt!(
                    "Modification of read-only variable \"%ls\" is not allowed\n",
                    name
                ));
                return false;
            }
        }
        return true;
    }
}

#[derive(Default)]
struct Match {
    all: bool,
    entire: bool,
    groups_only: bool,
    ignore_case: bool,
    invert_match: bool,
    quiet: bool,
    regex: bool,
    index: bool,
    pattern: WString,
}

impl SubCmdOptions for Match {
    const LONG_OPTIONS: &'static [woption<'static>] = &[
        wopt(L!("all"), woption_argument_t::no_argument, 'a'),
        wopt(L!("entire"), woption_argument_t::no_argument, 'e'),
        wopt(L!("groups-only"), woption_argument_t::no_argument, 'g'),
        wopt(L!("ignore-case"), woption_argument_t::no_argument, 'i'),
        wopt(L!("invert"), woption_argument_t::no_argument, 'v'),
        wopt(L!("quiet"), woption_argument_t::no_argument, 'q'),
        wopt(L!("regex"), woption_argument_t::no_argument, 'r'),
        wopt(L!("index"), woption_argument_t::no_argument, 'n'),
    ];
    const SHORT_OPTIONS: &'static wstr = L!(":aegivqrn");
}

impl SubCmdHandler for Match {
    fn take_args(
        &mut self,
        optind: &mut usize,
        args: &[&wstr],
        streams: &mut io_streams_t,
    ) -> Option<c_int> {
        let cmd = args[0];
        let Some(arg) = args.get(*optind).copied() else {
               string_error!(streams, BUILTIN_ERR_ARG_COUNT0, cmd);
               return STATUS_INVALID_ARGS;
           };
        *optind += 1;
        self.pattern = arg.to_owned();
        STATUS_CMD_OK
    }
    fn parse_options(&mut self, _optarg: Option<&wstr>, c: char) -> Result<(), ParseError> {
        match c {
            'a' => self.all = true,
            'e' => self.entire = true,
            'g' => self.groups_only = true,
            'i' => self.ignore_case = true,
            'v' => self.invert_match = true,
            'q' => self.quiet = true,
            'r' => self.regex = true,
            'n' => self.index = true,
            _ => return Err(ParseError::UnknownOption),
        }
        return Ok(());
    }

    fn handle(
        &mut self,
        parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int> {
        let cmd = args[0];

        if self.entire && self.index {
            streams.err.append(wgettext_fmt!(
                BUILTIN_ERR_COMBO2,
                cmd,
                "--entire and --index are mutually exclusive"
            ));
            return STATUS_INVALID_ARGS;
        }

        if self.invert_match && self.groups_only {
            streams.err.append(wgettext_fmt!(
                BUILTIN_ERR_COMBO2,
                cmd,
                "--invert and --groups-only are mutually exclusive"
            ));
            return STATUS_INVALID_ARGS;
        }

        if self.entire && self.groups_only {
            streams.err.append(wgettext_fmt!(
                BUILTIN_ERR_COMBO2,
                cmd,
                "--entire and --groups-only are mutually exclusive"
            ));
            return STATUS_INVALID_ARGS;
        }

        let mut matcher = if !self.regex {
            let mut wcpattern = parse_util_unescape_wildcards(&self.pattern);
            if self.ignore_case {
                wcpattern = wcpattern.to_lowercase();
            }
            if self.entire {
                if !wcpattern.is_empty() {
                    if wcpattern.char_at(0) != ANY_STRING {
                        wcpattern = iter::once(ANY_STRING).chain(wcpattern.chars()).collect();
                    }
                    if wcpattern.char_at(wcpattern.len() - 1) != ANY_STRING {
                        wcpattern.push(ANY_STRING);
                    }
                } else {
                    wcpattern.push(ANY_STRING);
                }
            }
            StringMatcher::WildCard {
                pattern: wcpattern,
                total_matched: 0,
                opts: self,
            }
        } else {
            let Some(regex) = try_compile_regex(&self.pattern, self.ignore_case, cmd, streams) else {
                    return STATUS_INVALID_ARGS;
            };
            if !Self::validate_capture_group_names(regex.capture_names(), streams) {
                return STATUS_INVALID_ARGS;
            }
            let first_match_captures = regex
                .capture_names()
                .iter()
                .filter_map(|name| name.as_ref().map(|n| (n.to_owned(), Vec::new())))
                .collect();
            StringMatcher::Regex {
                regex: Box::new(regex),
                total_matched: 0,
                first_match_captures,
                opts: self,
            }
        };

        let mut iter = Arguments::new(args, optind, true);
        while let Some(arg) = iter.next(streams) {
            if let Err(e) = matcher.report_matches(arg.as_ref(), streams) {
                FLOG!(error, "pcre2_match unexpected error:", e.error_message())
            }
            if self.quiet && matcher.match_count() > 0 {
                break;
            }
        }

        let match_count = matcher.match_count();

        if let StringMatcher::Regex {
            first_match_captures,
            ..
        } = matcher
        {
            let vars = parser.get_vars();
            for (name, vals) in first_match_captures.into_iter() {
                vars.set(&WString::from(name), EnvMode::DEFAULT, vals);
            }
        }

        if match_count > 0 {
            STATUS_CMD_OK
        } else {
            STATUS_CMD_ERROR
        }
    }
}

struct Pad {
    char_to_pad: char,
    pad_char_width: i32,
    pad_from: Direction,
    width: i32,
}

impl Default for Pad {
    fn default() -> Self {
        Self {
            char_to_pad: ' ',
            pad_char_width: 1,
            pad_from: Direction::Left,
            width: 0,
        }
    }
}

#[derive(Default, PartialEq)]
enum Direction {
    #[default]
    Left,
    Right,
}

impl SubCmdOptions for Pad {
    const LONG_OPTIONS: &'static [woption<'static>] = &[
        wopt(L!("quiet"), woption_argument_t::no_argument, 'q'),
        wopt(L!("right"), woption_argument_t::no_argument, 'r'),
        wopt(L!("width"), woption_argument_t::required_argument, 'w'),
    ];
    const SHORT_OPTIONS: &'static wstr = L!(":c:qrw:");
}

impl SubCmdHandler for Pad {
    fn parse_options(&mut self, optarg: Option<&wstr>, c: char) -> Result<(), ParseError> {
        match c {
            'c' => {
                let optarg = optarg.expect("option -c requires an argument");
                if optarg.len() != 1 {
                    return Err(ParseError::InvalidArgs(
                        "Padding should be a single character",
                    ));
                }
                let pad_char_width = fish_wcswidth(optarg.slice_to(1));
                // can we ever have negative width?
                if pad_char_width == 0 {
                    return Err(ParseError::InvalidArgs(
                        "Invalid padding character of width zero",
                    ));
                }
                self.pad_char_width = pad_char_width;
                self.char_to_pad = optarg.char_at(0);
            }
            'r' => self.pad_from = Direction::Right,
            'w' => {
                let optarg = optarg.expect("option --width requires an argument");
                self.width = match fish_wcstol(optarg) {
                    Ok(w) if w >= 0 => w as i32,
                    Ok(_) => return Err(ParseError::InvalidArgs("Invalid width")),
                    Err(_) => return Err(ParseError::NotANumber),
                }
            }
            _ => return Err(ParseError::UnknownOption),
        }
        return Ok(());
    }

    fn handle<'args>(
        &mut self,
        _parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&'args wstr],
    ) -> Option<c_int> {
        let mut max_width = 0i32;
        let mut inputs: Vec<(Cow<'args, wstr>, i32)> = Vec::new();

        let mut iter = Arguments::new(args, optind, true);
        while let Some(arg) = iter.next(streams) {
            let width = width_without_escapes(&arg, 0);
            max_width = max_width.max(width);
            inputs.push((arg, width));
        }

        let wants_newline = inputs
            .last()
            .map_or(false, |(input, _)| !input.ends_with('\n'));
        let pad_width = max_width.max(self.width);

        for (input, width) in inputs {
            use std::iter::repeat;

            let pad = (pad_width - width) / self.pad_char_width;
            let remaining_width = (pad_width - width) % self.pad_char_width;
            let mut padded: WString = match self.pad_from {
                Direction::Left => repeat(self.char_to_pad)
                    .take(pad as usize)
                    .chain(repeat(' ').take(remaining_width as usize))
                    .chain(input.chars())
                    .collect(),
                Direction::Right => input
                    .chars()
                    .chain(repeat(' ').take(remaining_width as usize))
                    .chain(repeat(self.char_to_pad).take(pad as usize))
                    .collect(),
            };

            if wants_newline {
                padded.push('\n');
            }

            streams.out.append(padded);
        }

        STATUS_CMD_OK
    }
}

struct Split {
    quiet: bool,
    split_from: Direction,
    max: usize,
    no_empty: bool,
    fields: Fields,
    allow_empty: bool,
    is_split0: bool,
    sep: WString,
}

impl Default for Split {
    fn default() -> Self {
        Self {
            quiet: false,
            split_from: Direction::Left,
            max: usize::MAX,
            no_empty: false,
            fields: Fields(Vec::new()),
            allow_empty: false,
            is_split0: false,
            sep: WString::from("\0"),
        }
    }
}

#[repr(transparent)]
struct Fields(Vec<usize>);

// we have a newtype just for the sake of implementing TryFrom
impl Deref for Fields {
    type Target = Vec<usize>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

enum FieldParseError {
    /// Unable to parse as integer
    Number,
    /// One of the ends in a range is either too big or small
    Range,
    /// The field is a valid number but outside of the allowed range
    Field,
}

impl TryFrom<&wstr> for Fields {
    type Error = FieldParseError;

    /// FIELDS is a comma-separated string of field numbers and/or spans.
    /// Each field is one-indexed.
    fn try_from(value: &wstr) -> Result<Self, Self::Error> {
        fn parse_field(f: &[char]) -> Result<Vec<usize>, FieldParseError> {
            use FieldParseError::*;
            let mut range = f.split(|&x| x == '-');
            let range: Vec<usize> = match (range.next(), range.next()) {
                (Some(_), None) => match fish_wcstol(wstr::from_char_slice(f)) {
                    Ok(n) if n >= 1 => vec![n as usize - 1],
                    Ok(_) => return Err(Field),
                    _ => return Err(Number),
                },
                (Some(s), Some(e)) => {
                    let start = match fish_wcstol(wstr::from_char_slice(s)) {
                        Ok(n) if n >= 1 => n as usize,
                        Ok(_) => return Err(Range),
                        _ => return Err(Number),
                    };
                    let end = match fish_wcstol(wstr::from_char_slice(e)) {
                        Ok(n) if n >= 1 => n as usize,
                        Ok(_) => return Err(Range),
                        _ => return Err(Number),
                    };
                    if start <= end {
                        // we store as 0-indexed, but the range is 1-indexed
                        (start - 1..end).collect()
                    } else {
                        // this is for some reason allowed
                        (end - 1..start).rev().collect()
                    }
                }
                _ => unreachable!("split() should always at least return an empty slice"),
            };
            Ok(range)
        }

        let fields = value.as_char_slice().split(|&x| x == ',').map(parse_field);

        let mut indices = Vec::new();
        for field in fields {
            indices.extend(field?);
        }

        Ok(Self(indices))
    }
}

impl SubCmdOptions for Split {
    const LONG_OPTIONS: &'static [woption<'static>] = &[
        wopt(L!("quiet"), woption_argument_t::no_argument, 'q'),
        wopt(L!("right"), woption_argument_t::no_argument, 'r'),
        wopt(L!("max"), woption_argument_t::required_argument, 'm'),
        wopt(L!("no-empty"), woption_argument_t::no_argument, 'n'),
        wopt(L!("fields"), woption_argument_t::required_argument, 'f'),
        // FIXME: allow-empty is not documented
        wopt(L!("allow-empty"), woption_argument_t::no_argument, 'a'),
    ];
    const SHORT_OPTIONS: &'static wstr = L!(":qrm:nf:a");
}

impl SubCmdHandler for Split {
    fn take_args(
        &mut self,
        optind: &mut usize,
        args: &[&wstr],
        streams: &mut io_streams_t,
    ) -> Option<c_int> {
        if self.is_split0 {
            return STATUS_CMD_OK;
        }
        let Some(arg) = args.get(*optind).copied() else {
            string_error!(streams, BUILTIN_ERR_ARG_COUNT0, args[0]);
            return STATUS_INVALID_ARGS;
        };
        *optind += 1;
        self.sep = arg.to_owned();
        return STATUS_CMD_OK;
    }
    fn parse_options(&mut self, optarg: Option<&wstr>, c: char) -> Result<(), ParseError> {
        match c {
            'q' => self.quiet = true,
            'r' => self.split_from = Direction::Right,
            'm' => {
                let optarg = optarg.expect("option --max requires an argument");
                self.max = match fish_wcstol(optarg) {
                    Ok(n) if n >= 0 => n as usize,
                    Ok(_) => return Err(ParseError::InvalidArgs("Invalid max value")),
                    Err(_) => return Err(ParseError::NotANumber),
                };
            }
            'n' => self.no_empty = true,
            'f' => {
                let optarg = optarg.expect("option --fields requires an argument");
                self.fields = match optarg.try_into() {
                    Ok(f) => f,
                    Err(FieldParseError::Number) => return Err(ParseError::NotANumber),
                    Err(FieldParseError::Range) => {
                        return Err(ParseError::InvalidArgs("Invalid range value for field"))
                    }
                    Err(FieldParseError::Field) => {
                        return Err(ParseError::InvalidArgs("Invalid range value for field"))
                    }
                };
            }
            'a' => self.allow_empty = true,
            _ => return Err(ParseError::UnknownOption),
        }
        return Ok(());
    }

    fn handle(
        &mut self,
        _parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int> {
        if self.fields.is_empty() && self.allow_empty {
            streams.err.append(wgettext_fmt!(
                BUILTIN_ERR_COMBO2,
                args[0],
                "--allow-empty is only valid with --fields"
            ));
            return STATUS_INVALID_ARGS;
        }

        let sep = &self.sep;
        // this can technically be changed to a Cow<'args, wstr>, but then split_about must use Cow
        let mut all_splits: Vec<Vec<WString>> = Vec::new();
        let mut split_count = 0usize;
        let mut arg_count = 0usize;

        let mut iter = Arguments::new(args, optind, !self.is_split0);
        while let Some(arg) = iter.next(streams) {
            let splits: Vec<WString> = match self.split_from {
                Direction::Right => {
                    let mut rev = arg.into_owned();
                    rev.as_char_slice_mut().reverse();
                    split_about(&rev, sep, self.max, self.no_empty)
                        .into_iter()
                        .map(|s| s.to_owned())
                        .collect()
                }
                Direction::Left => split_about(&arg, sep, self.max, self.no_empty)
                    .into_iter()
                    .map(|s| s.to_owned())
                    .collect(),
            };

            // If we're quiet, we return early if we've found something to split.
            if self.quiet && splits.len() > 1 {
                return STATUS_CMD_OK;
            }
            split_count += splits.len();
            arg_count += 1;
            all_splits.push(splits);
        }

        if self.quiet {
            return if split_count > arg_count {
                STATUS_CMD_OK
            } else {
                STATUS_CMD_ERROR
            };
        }

        for mut splits in all_splits {
            // If we are from the right, split_about gave us reversed strings, in reversed order!
            if self.split_from == Direction::Right {
                for split in splits.iter_mut() {
                    split.as_char_slice_mut().reverse();
                }
                splits.reverse();
            }

            if self.is_split0 && !splits.is_empty() {
                // split0 ignores a trailing "", so a\0b\0 is two elements.
                // In contrast to split, where a\nb\n is three - "a", "b" and "".
                //
                // Remove the last element if it is empty.
                if splits.last().unwrap().is_empty() {
                    splits.pop();
                }
            }
            if !self.fields.is_empty() {
                // Print nothing and return error if any of the supplied
                // fields do not exist, unless `--allow-empty` is used.
                if !self.allow_empty {
                    for field in self.fields.iter() {
                        // we already have checked the start
                        if *field >= splits.len() {
                            return STATUS_CMD_ERROR;
                        }
                    }
                }
                for field in self.fields.iter() {
                    if let Some(val) = splits.get(*field) {
                        streams
                            .out
                            .append_with_separation(val, SeparationType::explicitly, true);
                    }
                }
            } else {
                for split in &splits {
                    streams
                        .out
                        .append_with_separation(split, SeparationType::explicitly, true);
                }
            }
        }

        // We split something if we have more split values than args.
        return if split_count > arg_count {
            STATUS_CMD_OK
        } else {
            STATUS_CMD_ERROR
        };
    }
}

#[derive(Default)]
struct Repeat {
    count: usize,
    max: usize,
    quiet: bool,
    no_newline: bool,
}

impl SubCmdOptions for Repeat {
    const LONG_OPTIONS: &'static [woption<'static>] = &[
        wopt(L!("count"), woption_argument_t::required_argument, 'n'),
        wopt(L!("max"), woption_argument_t::required_argument, 'm'),
        wopt(L!("quiet"), woption_argument_t::no_argument, 'q'),
        wopt(L!("no-newline"), woption_argument_t::no_argument, 'N'),
    ];
    const SHORT_OPTIONS: &'static wstr = L!(":n:m:qN");
}

impl SubCmdHandler for Repeat {
    fn parse_options(&mut self, optarg: Option<&wstr>, c: char) -> Result<(), ParseError> {
        match c {
            'n' => {
                let optarg = optarg.expect("option --count requires an argument");
                self.count = match fish_wcstol(optarg) {
                    Ok(n) if n >= 0 => n as usize,
                    Ok(_) => return Err(ParseError::InvalidArgs("count value")),
                    Err(_) => return Err(ParseError::NotANumber),
                }
            }
            'm' => {
                let optarg = optarg.expect("option --max requires an argument");
                self.max = match fish_wcstol(optarg) {
                    Ok(m) if m >= 0 => m as usize,
                    Ok(_) => return Err(ParseError::InvalidArgs("max value")),
                    Err(_) => return Err(ParseError::NotANumber),
                }
            }
            'q' => self.quiet = true,
            'N' => self.no_newline = true,
            _ => return Err(ParseError::UnknownOption),
        }
        return Ok(());
    }

    fn handle(
        &mut self,
        _parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int> {
        if self.max == 0 && self.count == 0 {
            // XXX: This used to be allowed, but returned 1.
            // Keep it that way for now instead of adding an error.
            // streams.err.append(L"Count or max must be greater than zero");
            return STATUS_CMD_ERROR;
        }

        let mut all_empty = true;
        let mut first = true;

        let mut iter = Arguments::new(args, optind, true);
        while let Some(w) = iter.next(streams) {
            if w.is_empty() {
                continue;
            }

            all_empty = false;

            if self.quiet {
                // Early out if we can - see #7495.
                return STATUS_CMD_OK;
            }

            if !first {
                streams.out.push('\n');
            }
            first = false;

            // The maximum size of the string is either the "max" characters,
            // or it's the "count" repetitions, whichever ends up lower.
            let max = if self.max == 0 || (self.count > 0 && w.len() * self.count < self.max) {
                w.len() * self.count
            } else {
                self.max
            };

            // Reserve a string to avoid writing constantly.
            // The 1500 here is a total gluteal extraction, but 500 seems to perform slightly worse.
            let chunk_size = 1500;
            // The + word length is so we don't have to hit the chunk size exactly,
            // which would require us to restart in the middle of the string.
            // E.g. imagine repeating "12345678". The first chunk is hit after a last "1234",
            // so we would then have to restart by appending "5678", which requires a substring.
            // So let's not bother.
            //
            // Unless of course we don't even print the entire word, in which case we just need max.
            let mut chunk = WString::with_capacity(max.min(chunk_size + w.len()));

            let mut i = max;
            loop {
                if i >= w.len() {
                    chunk.push_utfstr(&w);
                } else {
                    chunk.push_utfstr(w.slice_to(i));
                    break;
                }

                if chunk.len() >= chunk_size {
                    // We hit the chunk size, write it repeatedly until we can't anymore.
                    streams.out.append(&chunk);
                    while i >= chunk.len() {
                        streams.out.append(&chunk);
                        // We can easily be asked to write *a lot* of data,
                        // so we need to check every so often if the pipe has been closed.
                        // If we didn't, running `string repeat -n LARGENUMBER foo | pv`
                        // and pressing ctrl-c seems to hang.
                        if streams.out.flush_and_check_error() != STATUS_CMD_OK.unwrap() {
                            return STATUS_CMD_ERROR;
                        }
                        i -= chunk.len();
                    }
                    chunk.clear();
                }

                let Some(new_i) = i.checked_sub(w.len()) else {
                    break
                };
                i = new_i;
            }

            // Flush the remainder.
            if !chunk.is_empty() {
                streams.out.append(&chunk);
            }
        }

        // Historical behavior is to never append a newline if all strings were empty.
        if !self.quiet && !self.no_newline && !all_empty && iter.want_newline() {
            streams.out.push('\n');
        }

        if all_empty {
            STATUS_CMD_ERROR
        } else {
            STATUS_CMD_OK
        }
    }
}

enum StringReplacer<'args, 'opts> {
    Regex {
        replacement: WString,
        regex: Box<Regex>,
        opts: &'opts Replace,
    },
    Literal {
        pattern: Cow<'args, wstr>,
        replacement: Cow<'args, wstr>,
        opts: &'opts Replace,
    },
}

impl<'args, 'opts> StringReplacer<'args, 'opts> {
    fn interpret_escape(arg: &'args wstr) -> Option<WString> {
        use crate::common::read_unquoted_escape;

        let mut result: WString = WString::with_capacity(arg.len());
        let mut cursor = arg;
        while !cursor.is_empty() {
            if cursor.char_at(0) == '\\' {
                if let Some(escape_len) = read_unquoted_escape(cursor, &mut result, true, false) {
                    cursor = cursor.slice_from(escape_len);
                } else {
                    // invalid escape
                    return None;
                }
            } else {
                result.push(cursor.char_at(0));
                cursor = cursor.slice_from(1);
            }
        }
        return Some(result);
    }

    fn new(
        pattern: &'args wstr,
        replacement: &'args wstr,
        opts: &'opts Replace,
        cmd: &wstr,
        streams: &mut io_streams_t,
    ) -> Option<Self> {
        if opts.regex {
            let Some(regex) = try_compile_regex(pattern, opts.ignore_case, cmd, streams) else {
                return None;
            };
            let replacement = if feature_test(FeatureFlag::string_replace_backslash) {
                replacement.to_owned()
            } else {
                Self::interpret_escape(replacement)?
            };
            Some(Self::Regex {
                replacement,
                regex: Box::new(regex),
                opts,
            })
        } else {
            Some(if opts.ignore_case {
                Self::Literal {
                    pattern: Cow::Owned(pattern.to_lowercase()),
                    replacement: Cow::Owned(replacement.to_lowercase()),
                    opts,
                }
            } else {
                Self::Literal {
                    pattern: Cow::Borrowed(pattern),
                    replacement: Cow::Borrowed(replacement),
                    opts,
                }
            })
        }
    }

    /// Return None if failed, inner bool indicates if something was replaced
    /// The string is the result of the replacement
    fn replace<'a>(&self, arg: Cow<'a, wstr>) -> Result<(bool, Cow<'a, wstr>), pcre2::Error> {
        match self {
            StringReplacer::Regex {
                replacement,
                regex,
                opts,
            } => {
                if replacement.is_empty() {
                    return Ok((false, arg));
                }

                let res = if opts.all {
                    regex.replace_all(arg.as_char_slice(), replacement.as_char_slice(), true)
                } else {
                    regex.replace(arg.as_char_slice(), replacement.as_char_slice(), true)
                }?;

                let res = match res {
                    Cow::Borrowed(_slice_of_arg) => (false, arg),
                    Cow::Owned(s) => (true, Cow::Owned(WString::from_chars(s))),
                };
                return Ok(res);
            }
            StringReplacer::Literal {
                pattern,
                replacement,
                opts,
            } => {
                // a premature optimization would be to alloc larger if we replacement.len() > pattern.len()
                let mut result = WString::with_capacity(arg.len());

                let arg = if opts.ignore_case {
                    Cow::Owned(arg.to_lowercase())
                } else {
                    arg
                };

                let mut offset = 0;
                while let Some(idx) = arg[offset..].find(pattern.as_char_slice()) {
                    result.push_utfstr(&arg[offset..offset + idx]);
                    result.push_utfstr(&replacement);
                    offset += idx + pattern.len();
                    if !opts.all {
                        break;
                    }
                }
                if offset == 0 {
                    return Ok((false, arg));
                }
                result.push_utfstr(&arg[offset..]);

                Ok((true, Cow::Owned(result)))
            }
        }
    }
}

#[derive(Default)]
struct Replace {
    all: bool,
    filter: bool,
    ignore_case: bool,
    quiet: bool,
    regex: bool,
    pattern: WString,
    replacement: WString,
}

impl SubCmdOptions for Replace {
    const LONG_OPTIONS: &'static [woption<'static>] = &[
        wopt(L!("all"), woption_argument_t::no_argument, 'a'),
        wopt(L!("filter"), woption_argument_t::no_argument, 'f'),
        wopt(L!("ignore-case"), woption_argument_t::no_argument, 'i'),
        wopt(L!("quiet"), woption_argument_t::no_argument, 'q'),
        wopt(L!("regex"), woption_argument_t::no_argument, 'r'),
    ];
    const SHORT_OPTIONS: &'static wstr = L!(":afiqr");
}

impl SubCmdHandler for Replace {
    fn take_args(
        &mut self,
        optind: &mut usize,
        args: &[&wstr],
        streams: &mut io_streams_t,
    ) -> Option<c_int> {
        let cmd = args[0];
        let Some(pattern) = args.get(*optind).copied() else {
            string_error!(streams, BUILTIN_ERR_ARG_COUNT0, cmd);
            return STATUS_INVALID_ARGS;
        };
        *optind += 1;
        let Some(replacement) = args.get(*optind).copied() else {
            string_error!(streams, BUILTIN_ERR_ARG_COUNT1, cmd, 1);
            return STATUS_INVALID_ARGS;
        };
        *optind += 1;

        self.pattern = pattern.to_owned();
        self.replacement = replacement.to_owned();
        return STATUS_CMD_OK;
    }
    fn parse_options(&mut self, _optarg: Option<&wstr>, c: char) -> Result<(), ParseError> {
        match c {
            'a' => self.all = true,
            'f' => self.filter = true,
            'i' => self.ignore_case = true,
            'q' => self.quiet = true,
            'r' => self.regex = true,
            _ => return Err(ParseError::UnknownOption),
        }
        return Ok(());
    }

    fn handle(
        &mut self,
        _parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int> {
        let cmd = args[0];

        let Some(replacer) = StringReplacer::new(&self.pattern, &self.replacement, self, cmd, streams) else {
            // failed to init regex
            return STATUS_INVALID_ARGS;
        };

        let mut replace_count = 0;

        let mut iter = Arguments::new(args, optind, true);
        while let Some(arg) = iter.next(streams) {
            let (replaced, result) = match replacer.replace(arg) {
                Ok(x) => x,
                Err(e) => {
                    string_error!(
                        streams,
                        "%ls: Regular expression substitute error: %ls\n",
                        cmd,
                        e.error_message()
                    );
                    return STATUS_INVALID_ARGS;
                }
            };
            replace_count += replaced as usize;

            if !self.quiet && (!self.filter || replaced) {
                streams.out.append(result);
                if iter.want_newline() {
                    streams.out.push('\n');
                }
            }

            if self.quiet && replace_count > 0 {
                return STATUS_CMD_OK;
            }
        }

        if replace_count > 0 {
            STATUS_CMD_OK
        } else {
            STATUS_CMD_ERROR
        }
    }
}

struct Shorten {
    chars_to_shorten: WString,
    max: Option<usize>,
    no_newline: bool,
    quiet: bool,
    direction: Direction,
}

impl Default for Shorten {
    fn default() -> Self {
        Self {
            chars_to_shorten: get_ellipsis_str().to_owned(),
            max: None,
            no_newline: false,
            quiet: false,
            direction: Direction::Right,
        }
    }
}

impl SubCmdOptions for Shorten {
    // TODO
    const LONG_OPTIONS: &'static [woption<'static>] = &[
        wopt(L!("char"), woption_argument_t::required_argument, 'c'),
        wopt(L!("max"), woption_argument_t::required_argument, 'm'),
        wopt(L!("no-newline"), woption_argument_t::no_argument, 'N'),
        wopt(L!("left"), woption_argument_t::no_argument, 'l'),
        wopt(L!("quiet"), woption_argument_t::no_argument, 'q'),
    ];
    const SHORT_OPTIONS: &'static wstr = L!(":c:m:Nlq");
}

impl SubCmdHandler for Shorten {
    fn parse_options(&mut self, optarg: Option<&wstr>, c: char) -> Result<(), ParseError> {
        match c {
            'c' => {
                self.chars_to_shorten = optarg
                    .expect("option --char requires an argument")
                    .to_owned()
            }
            'm' => {
                let optarg = optarg.expect("option --max requires an argument");
                self.max = match fish_wcstol(optarg) {
                    Ok(n) if n >= 0 => Some(n as usize),
                    Ok(_) => return Err(ParseError::InvalidArgs("Invalid max value")),
                    Err(_) => return Err(ParseError::NotANumber),
                };
            }
            'N' => self.no_newline = true,
            'l' => self.direction = Direction::Left,
            'q' => self.quiet = true,
            _ => return Err(ParseError::UnknownOption),
        }
        return Ok(());
    }

    fn handle(
        &mut self,
        _parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int> {
        let mut min_width = usize::MAX;
        let mut inputs = Vec::new();
        let mut ell = self.chars_to_shorten.as_utfstr();

        let mut iter = Arguments::new(args, optind, true);

        if Some(0) == self.max {
            // Special case: Max of 0 means no shortening.
            // This makes this more reusable, so you don't need special-cases like
            //
            // if test $shorten -gt 0
            //     string shorten -m $shorten whatever
            // else
            //     echo whatever
            // end
            while let Some(arg) = iter.next(streams) {
                streams.out.append(arg);
                streams.out.append1('\n');
            }
            return STATUS_CMD_OK;
        }

        // let mut iter = Arguments::new(args, optind, true);
        while let Some(arg) = iter.next(streams) {
            // Visible width only makes sense line-wise.
            // So either we have no-newlines (which means we shorten on the first newline),
            // or we handle the lines separately.
            let mut splits = split_string(&arg, '\n').into_iter();
            if self.no_newline && splits.len() > 1 {
                let mut s = match self.direction {
                    Direction::Left => splits.last(),
                    Direction::Right => splits.next(),
                }
                .unwrap();
                s.push_utfstr(ell);
                let width = width_without_escapes(&s, 0);

                if width > 0 && (width as usize) < min_width {
                    min_width = width as usize;
                }
                inputs.push(s);
            } else {
                for s in splits {
                    let width = width_without_escapes(&s, 0);
                    if width > 0 && (width as usize) < min_width {
                        min_width = width as usize;
                    }
                    inputs.push(s);
                }
            }
        }

        let ourmax: usize = self.max.unwrap_or(min_width);

        // TODO: Can we have negative width

        let ell_width: i32 = {
            let w = fish_wcswidth(ell);
            if w > ourmax as i32 {
                // If we can't even print our ellipsis, we substitute nothing,
                // truncating instead.
                ell = L!("");
                0
            } else {
                w
            }
        };

        let mut nsub = 0usize;
        // We could also error out here if the width of our ellipsis is larger
        // than the target width.
        // That seems excessive - specifically because the ellipsis on LANG=C
        // is "..." (width 3!).

        let skip_escapes = |l: &wstr, pos: usize| -> usize {
            let mut totallen = 0usize;
            while l.char_at(pos + totallen) == '\x1B' {
                let Some(len) = escape_code_length(l.slice_from(pos + totallen)) else {
                    break;
                };
                totallen += len;
            }
            totallen
        };

        for line in inputs {
            let mut pos = 0usize;
            let mut max = 0usize;
            // Collect how much of the string we can use without going over the maximum.
            if self.direction == Direction::Left {
                // Our strategy for keeping from the end.
                // This is rather unoptimized - actually going *backwards* from the end
                // is extremely tricky because we would have to subtract escapes again.
                // Also we need to avoid hacking combiners into bits.
                // This should work for most cases considering the combiners typically have width 0.
                let mut out = L!("");
                while pos < line.len() {
                    let w = width_without_escapes(&line, pos);
                    // If we're at the beginning and it fits, we sits.
                    //
                    // Otherwise we require it to fit the ellipsis
                    if (w <= ourmax as i32 && pos == 0) || (w + ell_width <= ourmax as i32) {
                        out = line.slice_from(pos);
                        break;
                    }

                    pos += skip_escapes(&line, pos).max(1);
                }
                if self.quiet && pos != 0 {
                    return STATUS_CMD_OK;
                }

                let output = match pos {
                    0 => line,
                    _ => {
                        nsub += 1;
                        let mut res = WString::with_capacity(ell.len() + out.len());
                        res.push_utfstr(ell);
                        res.push_utfstr(out);
                        res
                    }
                };
                streams.out.append(output);
                streams.out.append1('\n');
                continue;
            } else {
                /* Direction::Right */
                // Going from the left.
                // This is somewhat easier.
                while max <= ourmax && pos < line.len() {
                    pos += skip_escapes(&line, pos);
                    let w = fish_wcwidth(line.char_at(pos));
                    if w <= 0 || max as i32 + w + ell_width <= ourmax as i32 {
                        // If it still fits, even if it is the last, we add it.
                        max += w as usize;
                        pos += 1;
                    } else {
                        // We're at the limit, so see if the entire string fits.
                        let mut max2: i32 = max as i32 + w;
                        let mut pos2 = pos + 1;
                        while pos2 < line.len() {
                            pos2 += skip_escapes(&line, pos2);
                            max2 += fish_wcwidth(line.char_at(pos2));
                            pos2 += 1;
                        }

                        if max2 <= ourmax as i32 {
                            // We're at the end and everything fits,
                            // no ellipsis.
                            pos = pos2;
                        }
                        break;
                    }
                }
            }

            if self.quiet && pos != line.len() {
                return STATUS_CMD_OK;
            }

            if pos == line.len() {
                streams.out.append(line);
                streams.out.append1('\n');
                continue;
            }

            nsub += 1;
            let mut newl = line;
            newl.truncate(pos);
            newl.push_utfstr(ell);
            newl.push('\n');
            streams.out.append(newl);
        }

        // Return true if we have shortened something and false otherwise.
        if nsub > 0 {
            STATUS_CMD_OK
        } else {
            STATUS_CMD_ERROR
        }
    }
}

#[derive(Default)]
struct Sub {
    length: Option<usize>,
    quiet: bool,
    start: i64,
    end: Option<i64>,
}

impl SubCmdOptions for Sub {
    const LONG_OPTIONS: &'static [woption<'static>] = &[
        wopt(L!("length"), woption_argument_t::required_argument, 'l'),
        wopt(L!("start"), woption_argument_t::required_argument, 's'),
        wopt(L!("end"), woption_argument_t::required_argument, 'e'),
        wopt(L!("quiet"), woption_argument_t::no_argument, 'q'),
    ];
    const SHORT_OPTIONS: &'static wstr = L!(":l:qs:e:");
}

impl SubCmdHandler for Sub {
    fn parse_options(&mut self, optarg: Option<&wstr>, c: char) -> Result<(), ParseError> {
        match c {
            'l' => {
                let optarg = optarg.expect("option --length requires an argument");
                self.length = match fish_wcstol(optarg) {
                    Ok(n) if n >= 0 => Some(n as usize),
                    Ok(_) => return Err(ParseError::InvalidArgs("Invalid length value")),
                    Err(_) => return Err(ParseError::NotANumber),
                };
            }
            's' => {
                let optarg = optarg.expect("option --start requires an argument");
                self.start = match fish_wcstol(optarg) {
                    Ok(n) if n != 0 => n,
                    Ok(_) => return Err(ParseError::InvalidArgs("Invalid start value")),
                    Err(_) => return Err(ParseError::NotANumber),
                };
            }
            'e' => {
                let optarg = optarg.expect("option --end requires an argument");
                self.end = match fish_wcstol(optarg) {
                    Ok(n) if n != 0 => Some(n),
                    Ok(_) => return Err(ParseError::InvalidArgs("Invalid end value")),
                    Err(_) => return Err(ParseError::NotANumber),
                };
            }
            'q' => self.quiet = true,
            _ => return Err(ParseError::UnknownOption),
        }
        return Ok(());
    }

    fn handle(
        &mut self,
        _parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int> {
        let cmd = args[0];
        if self.length.is_some() && self.end.is_some() {
            streams.err.append(wgettext_fmt!(
                BUILTIN_ERR_COMBO2,
                cmd,
                L!("--end and --length are mutually exclusive")
            ));
            return STATUS_INVALID_ARGS;
        }

        let mut nsub = 0;
        let mut iter = Arguments::new(args, optind, true);
        while let Some(s) = iter.next(streams) {
            let len = s.len();
            let start: usize = match self.start {
                n @ 1.. => n - 1,
                0 => 0,
                n => (len as i64) + n,
            }
            .clamp(0, len as i64) as usize;

            let count = {
                let n = self
                    .end
                    .map(|e| match e {
                        // end can never be 0
                        n @ 1.. => n as usize,
                        n => (len as i64 + n).max(0) as usize,
                    })
                    .map(|n| n.checked_sub(start).unwrap_or_default());

                n.or(self.length).unwrap_or(len)
            };

            if !self.quiet {
                streams
                    .out
                    .append(&s[start..usize::min(start + count, s.len())]);
                if iter.want_newline() {
                    streams.out.push('\n');
                }
            }
            nsub += 1;
            if self.quiet {
                return STATUS_CMD_OK;
            }
        }

        if nsub > 0 {
            STATUS_CMD_OK
        } else {
            STATUS_CMD_ERROR
        }
    }
}

struct Trim {
    chars_to_trim: WString,
    left: bool,
    right: bool,
    quiet: bool,
}

impl Default for Trim {
    fn default() -> Self {
        Self {
            // from " \f\n\r\t\v"
            chars_to_trim: WString::from(" \x0C\n\r\x09\x0B"),
            left: false,
            right: false,
            quiet: false,
        }
    }
}

impl SubCmdOptions for Trim {
    const LONG_OPTIONS: &'static [woption<'static>] = &[
        wopt(L!("chars"), woption_argument_t::required_argument, 'c'),
        wopt(L!("left"), woption_argument_t::no_argument, 'l'),
        wopt(L!("right"), woption_argument_t::no_argument, 'r'),
        wopt(L!("quiet"), woption_argument_t::no_argument, 'q'),
    ];
    const SHORT_OPTIONS: &'static wstr = L!(":c:lrq");
}

impl SubCmdHandler for Trim {
    fn parse_options(&mut self, optarg: Option<&wstr>, c: char) -> Result<(), ParseError> {
        match c {
            'c' => {
                let optarg = optarg.expect("option --chars requires an argument");
                self.chars_to_trim = optarg.to_owned();
            }
            'l' => self.left = true,
            'r' => self.right = true,
            'q' => self.quiet = true,
            _ => return Err(ParseError::UnknownOption),
        }
        return Ok(());
    }

    fn handle(
        &mut self,
        _parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int> {
        // If neither left or right is specified, we do both.
        if !self.left && !self.right {
            self.left = true;
            self.right = true;
        }

        let mut ntrim = 0;

        let to_trim_end = |str: &wstr| -> usize {
            str.chars()
                .rev()
                .take_while(|&c| self.chars_to_trim.contains(c))
                .count()
        };

        let to_trim_start = |str: &wstr| -> usize {
            str.chars()
                .take_while(|&c| self.chars_to_trim.contains(c))
                .count()
        };

        let mut iter = Arguments::new(args, optind, true);
        while let Some(arg) = iter.next(streams) {
            let trim_start = self.left.then(|| to_trim_start(&arg)).unwrap_or(0);
            // collision is only an issue if the whole string is getting trimmed
            let trim_end = (self.right && trim_start != arg.len())
                .then(|| to_trim_end(&arg))
                .unwrap_or(0);

            ntrim += trim_start + trim_end;
            if !self.quiet {
                streams.out.append(&arg[trim_start..arg.len() - trim_end]);
                if iter.want_newline() {
                    streams.out.push('\n');
                }
            } else if ntrim > 0 {
                return STATUS_CMD_OK;
            }
        }

        if ntrim > 0 {
            STATUS_CMD_OK
        } else {
            STATUS_CMD_ERROR
        }
    }
}

#[derive(Default)]
struct Unescape {
    no_quoted: bool,
    style: UnescapeStringStyle,
}

impl SubCmdOptions for Unescape {
    const LONG_OPTIONS: &'static [woption<'static>] = &[
        wopt(L!("no-quoted"), woption_argument_t::no_argument, 'q'),
        wopt(L!("style"), woption_argument_t::required_argument, '\u{1}'),
    ];
    const SHORT_OPTIONS: &'static wstr = L!(":q");
}

impl SubCmdHandler for Unescape {
    fn parse_options(&mut self, optarg: Option<&wstr>, c: char) -> Result<(), ParseError> {
        match c {
            'q' => self.no_quoted = true,
            '\u{1}' => {
                let optarg = optarg.expect("option --style requires an argument");
                self.style = UnescapeStringStyle::try_from(optarg)
                    .map_err(|_| ParseError::InvalidArgs("escape style"))?;
            }
            _ => return Err(ParseError::UnknownOption),
        }
        return Ok(());
    }

    fn handle(
        &mut self,
        _parser: &mut parser_t,
        streams: &mut io_streams_t,
        optind: &mut usize,
        args: &mut [&wstr],
    ) -> Option<c_int> {
        let mut nesc = 0;
        let mut iter = Arguments::new(args, optind, true);
        while let Some(arg) = iter.next(streams) {
            if let Some(res) = unescape_string(&arg, self.style) {
                streams.out.append(res);
                if iter.want_newline() {
                    streams.out.push('\n');
                }
                nesc += 1;
            }
        }

        if nesc > 0 {
            STATUS_CMD_OK
        } else {
            STATUS_CMD_ERROR
        }
    }
}

struct Arguments<'args, 'iter> {
    args: &'iter [&'args wstr],
    argidx: &'iter mut usize,
    split_on_newline: bool,
    buffer: Vec<u8>,
    /// If set, we have consumed all of stdin and its last line is missing a newline character.
    /// This is an edge case -- we expect text input, which is conventionally terminated by a
    /// newline character. But if it isn't, we use this to avoid creating one out of thin air,
    /// to not corrupt input data.
    missing_trailing_newline: bool,
    reader: Option<BufReader<File>>,
}

impl Drop for Arguments<'_, '_> {
    fn drop(&mut self) {
        if let Some(r) = self.reader.take() {
            // we should not close stdin
            std::mem::forget(r.into_inner());
        }
    }
}

impl<'args, 'iter> Arguments<'args, 'iter> {
    fn new(args: &'iter [&'args wstr], argidx: &'iter mut usize, split_on_newline: bool) -> Self {
        Arguments {
            args,
            argidx,
            split_on_newline,
            buffer: Vec::new(),
            missing_trailing_newline: false,
            reader: None,
        }
    }

    /// Returns true if we should add a newline after printing output for the current item.
    /// This is only ever false in an edge case, namely after we have consumed stdin and the
    /// last line is missing a trailing newline.
    fn want_newline(&self) -> bool {
        !self.missing_trailing_newline
    }

    fn get_arg_stdin(&mut self, streams: &mut io_streams_t) -> Option<Cow<'args, wstr>> {
        assert!(
            streams.stdin_is_directly_redirected(),
            "should not be reading from stdin"
        );

        let reader = self.reader.get_or_insert_with(|| {
            let stdin_fd = streams
                .stdin_fd()
                .filter(|&fd| fd >= 0)
                .expect("should have a valid fd");
            // safety: stdin is already open, and not our responsibility to close it
            let fd = unsafe { File::from_raw_fd(stdin_fd) };
            BufReader::with_capacity(STRING_CHUNK_SIZE, fd)
        });

        // NOTE: C++ wrongly commented that read_blocked retries for EAGAIN
        let num_bytes = match self.split_on_newline {
            true => reader.read_until(b'\n', &mut self.buffer),
            false => reader.read_to_end(&mut self.buffer),
        }
        .ok()?;

        // to match behaviour of earlier versions
        if num_bytes == 0 {
            return None;
        }

        let mut parsed = str2wcstring(&self.buffer);

        if self.split_on_newline && parsed.char_at(parsed.len() - 1) == '\n' {
            // consumers do not expect to deal with the newline
            parsed.pop();
        } else {
            self.missing_trailing_newline = !self.split_on_newline;
        }

        let retval = Some(Cow::Owned(parsed));
        self.buffer.clear();
        retval
    }

    /// We don`t implement Iterator to avoid wrapping streams in a RefCell
    fn next(&mut self, streams: &mut io_streams_t) -> Option<Cow<'args, wstr>> {
        if streams.stdin_is_directly_redirected() {
            return self.get_arg_stdin(streams);
        }

        if *self.argidx >= self.args.len() {
            return None;
        }
        *self.argidx += 1;
        return Some(Cow::Borrowed(self.args[*self.argidx - 1]));
    }
}

/// The string builtin, for manipulating strings.
pub fn string(
    parser: &mut parser_t,
    streams: &mut io_streams_t,
    args: &mut [&wstr],
) -> Option<c_int> {
    let cmd = args[0];
    let argc = args.len();

    if argc <= 1 {
        streams
            .err
            .append(wgettext_fmt!(BUILTIN_ERR_MISSING_SUBCMD, cmd));
        builtin_print_error_trailer(parser, streams, cmd);
        return STATUS_INVALID_ARGS;
    }

    if args[1] == "-h" || args[1] == "--help" {
        builtin_print_help(parser, streams, cmd);
        return STATUS_CMD_OK;
    }

    let subcmd_name = args[1];

    let Some(mut subcmd) = SUBCOMMANDS.binary_search_by_key(&subcmd_name, |x| x.0).ok().map(|x| SUBCOMMANDS[x].1()) else {
        streams.err.append(wgettext_fmt!(
            BUILTIN_ERR_INVALID_SUBCMD,
            cmd,
            subcmd_name,
        ));
        builtin_print_error_trailer(parser, streams, cmd);
        return STATUS_INVALID_ARGS;
    };

    if argc >= 3 && (args[2] == "-h" || args[2] == "--help") {
        let string_dash_subcmd = WString::from(args[0]) + L!("-") + subcmd_name;
        builtin_print_help(parser, streams, &string_dash_subcmd);
        return STATUS_CMD_OK;
    }

    let args = &mut args[1..];

    let mut optind = 0;
    let retval = parse_opts(&mut subcmd, &mut optind, args, parser, streams);
    if retval != STATUS_CMD_OK {
        return retval;
    }

    let retval = subcmd.take_args(&mut optind, args, streams);
    if retval != STATUS_CMD_OK {
        return retval;
    }

    if streams.stdin_is_directly_redirected() && args.len() > optind {
        string_error!(streams, BUILTIN_ERR_TOO_MANY_ARGUMENTS, args[0]);
        return STATUS_INVALID_ARGS;
    }

    return subcmd.handle(parser, streams, &mut optind, args);
}
