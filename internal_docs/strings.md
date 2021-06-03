---
id: strings
title: Strings
---

## String types

fish-shell uses wide-character `wchar_t` strings. A `wchar_t` is 4 bytes on most platforms, with the exception of Cygwin Windows, where it is 2 bytes. A 2 byte wchar_t is not well supported.

There are two major string types: `wcstring` and `imstring`.

## wcstring

`wcstring` is an alias for `std::wstring`, the C++ STL string type. This is similar to `std::string` except the type is `wchar_t` instead of `char`.

`wcstring` is useful for strings that may need to be modified. For example, expanding variables uses `wcstring`, so that the string may be modified in-place, reusing its storage. `wcstring` is separately allocated except for very short strings (length < 3 on Linux, 4 on Mac). It does not share storage: copying a `wcstring` usually incurs an allocation.

Creating a `wcstring` from a string literal will incur a heap allocation:

    void func(const wcstring &s);
    func(L"this will trigger an allocation");

## imstring

`imstring` is a custom, immutable string type intended to address the performance problems of `wcstring`. `imstring` may reference a string literal, without copying it. It may also reference unowned data, and copy it lazily. Lastly, it may own its own string contents in a `shared_ptr`, so that multiple `imstring`s will share the same underlying storage.

`imstring` is a subclass of `string_view`, so you may use `string_view` functions like `find`, `compare`, etc.

`imstring` may be constructed from a string literal:

    imstring s = L"literal";

This will *not* trigger an allocation, even if copied. There is also the `_im` suffix to create a literal `imstring`:

    auto s = L"something"_im;

You may also construct an `imstring` by transferring ownership of a `wcstring`:

    wcstring stuff = L"thing";
    imstring s = std::move(stuff);

This avoids copying the string storage.

Lastly, you may initialize an `imstring` from a pointer or reference, without transferring ownership:

    wcstring stuff = L"thing";
    imstring s = stuff;

Here the `imstring` will reference the pointer.

### Pitfalls

It is possible to make an `imstring` dangle, similar to how you might with `string_view`.

Examples of invalid uses:

    imstring foo1() {
        wchar_t local[] = L"stuff";
        imstring res = local;
        return res;
    }

    imstring foo2() {
        wcstring local = L"stuff";
        imstring res = local;
        return res;
    }

Both of these will result in a dangling pointer, like they would with `string_view`.

However `imstring` is safe when copied. Note `string_view` would not be safe here.

    void foo3() {
        std::vector<imstring> vec;
        wcstring local = L"stuff";
        imstring res = local;
        vec.push_back(res); // copies the storage
    }

### Representation of invalid data

It may be that a command produces output which is not valid in the user's preferred encoding. In this case, fish will encode the invalid byte sequence in the Unicode private use area, starting with `ENCODE_DIRECT_BASE`. For example a single byte `0xFF` is invalid UTF-8; fish will encode this as `ENCODE_DIRECT_BASE + 0xFF`.
