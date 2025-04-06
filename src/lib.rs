pub mod iter;
mod error;
#[macro_use] pub mod macros;

use core::{fmt, mem, result, str};
use core::mem::MaybeUninit;
use error::Error;
use crate::iter::Bytes;

/// Determines if byte is a method token char.
///
/// > ```notrust
/// > token          = 1*tchar
/// >
/// > tchar          = "!" / "#" / "$" / "%" / "&" / "'" / "*"
/// >                / "+" / "-" / "." / "^" / "_" / "`" / "|" / "~"
/// >                / DIGIT / ALPHA
/// >                ; any VCHAR, except delimiters
/// > ```
#[inline]
fn is_method_token(b: u8) -> bool {
    match b {
        // For the majority case, this can be faster than the table lookup.
        b'A'..=b'Z' => true,
        _ => TOKEN_MAP[b as usize],
    }
}

// char codes to accept URI string.
// i.e. b'!' <= char and char != 127
// TODO: Make a stricter checking for URI string?
static URI_MAP: [bool; 256] = byte_map!(
    b'!'..=0x7e | 0x80..=0xFF
);

#[inline]
pub(crate) fn is_uri_token(b: u8) -> bool {
    URI_MAP[b as usize]
}

static TOKEN_MAP: [bool; 256] = byte_map!(
    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' |
    b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' |  b'*' | b'+' |
    b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
);

#[inline]
pub(crate) fn is_header_name_token(b: u8) -> bool {
    TOKEN_MAP[b as usize]
}


static HEADER_VALUE_MAP: [bool; 256] = byte_map!(
    b'\t' | b' '..=0x7e | 0x80..=0xFF
);


#[inline]
pub(crate) fn is_header_value_token(b: u8) -> bool {
    HEADER_VALUE_MAP[b as usize]
}


/// A Result of any parsing action.
///
/// If the input is invalid, an `Error` will be returned. Note that incomplete
/// data is not considered invalid, and so will not return an error, but rather
/// a `Ok(Status::Partial)`.
pub type Result<T> = result::Result<Status<T>, Error>;

/// The result of a successful parse pass.
///
/// `Complete` is used when the buffer contained the complete value.
/// `Partial` is used when parsing did not reach the end of the expected value,
/// but no invalid data was found.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Status<T> {
    /// The completed result.
    Complete(T),
    /// A partial result.
    Partial
}

impl<T> Status<T> {
    /// Convenience method to check if status is complete.
    #[inline]
    pub fn is_complete(&self) -> bool {
        match *self {
            Status::Complete(..) => true,
            Status::Partial => false
        }
    }

    /// Convenience method to check if status is partial.
    #[inline]
    pub fn is_partial(&self) -> bool {
        match *self {
            Status::Complete(..) => false,
            Status::Partial => true
        }
    }

    /// Convenience method to unwrap a Complete value. Panics if the status is
    /// `Partial`.
    #[inline]
    pub fn unwrap(self) -> T {
        match self {
            Status::Complete(t) => t,
            Status::Partial => panic!("Tried to unwrap Status::Partial")
        }
    }
}

/// Represents a parsed header.
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Header<'a> {
    /// The name portion of a header.
    ///
    /// A header name must be valid ASCII-US, so it's safe to store as a `&str`.
    pub name: &'a str,
    /// The value portion of a header.
    ///
    /// While headers **should** be ASCII-US, the specification allows for
    /// values that may not be, and so the value is stored as bytes.
    pub value: &'a [u8],
}

impl fmt::Debug for Header<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut f = f.debug_struct("Header");
        f.field("name", &self.name);
        if let Ok(value) = str::from_utf8(self.value) {
            f.field("value", &value);
        } else {
            f.field("value", &self.value);
        }
        f.finish()
    }
}

/// An empty header, useful for constructing a `Header` array to pass in for
/// parsing.
///
/// # Example
///
/// ```
/// let headers = [httparse::EMPTY_HEADER; 64];
/// ```
pub const EMPTY_HEADER: Header<'static> = Header { name: "", value: b"" };


/// A parsed Request.
///
/// The optional values will be `None` if a parse was not complete, and did not
/// parse the associated property. This allows you to inspect the parts that
/// could be parsed, before reading more, in case you wish to exit early.
///
/// # Example
///
/// ```no_run
/// let buf = b"GET /404 HTTP/1.1\r\nHost:";
/// let mut headers = [httparse::EMPTY_HEADER; 16];
/// let mut req = httparse::Request::new(&mut headers);
/// let res = req.parse(buf).unwrap();
/// if res.is_partial() {
///     match req.path {
///         Some(ref path) => {
///             // check router for path.
///             // /404 doesn't exist? we could stop parsing
///         },
///         None => {
///             // must read more and parse again
///         }
///     }
/// }
/// ```
#[derive(Debug, Eq, PartialEq)]
pub struct Request<'headers, 'buf> {
    /// The request method, such as `GET`.
    pub method: Option<&'buf str>,
    /// The request path, such as `/about-us`.
    pub path: Option<&'buf str>,
    /// The request minor version, such as `1` for `HTTP/1.1`.
    pub version: Option<u8>,
    /// The request headers.
    pub headers: &'headers mut [Header<'buf>]
}

impl<'h, 'b> Request<'h, 'b> {
    /// Creates a new Request, using a slice of headers you allocate.
    #[inline]
    pub fn new(headers: &'h mut [Header<'b>]) -> Request<'h, 'b> {
        Request {
            method: None,
            path: None,
            version: None,
            headers,
        }
    }

    fn parse(&mut self) {
    }
}

#[inline]
fn skip_empty_lines(bytes: &mut Bytes<'_>) -> Result<()> {
    loop {
        let b = bytes.peek();
        match b {
            Some(b'\r') => {
                // SAFETY: peeked and found `\r`, so it's safe to bump 1 pos
                unsafe { bytes.bump() };
                expect!(bytes.next() == b'\n' => Err(Error::NewLine));
            }
            Some(b'\n') => {
                // SAFETY: peeked and found `\n`, so it's safe to bump 1 pos
                unsafe {
                    bytes.bump();
                }
            }
            Some(..) => {
                bytes.slice();
                return Ok(Status::Complete(()));
            }
            None => return Ok(Status::Partial),
        }
    }
}

#[inline]
fn skip_spaces(bytes: &mut Bytes<'_>) -> Result<()> {
    loop {
        let b = bytes.peek();
        match b {
            Some(b' ') => {
                // SAFETY: peeked and found ` `, so it's safe to bump 1 pos
                unsafe { bytes.bump() };
            }
            Some(..) => {
                bytes.slice();
                return Ok(Status::Complete(()));
            }
            None => return Ok(Status::Partial),
        }
    }
}
