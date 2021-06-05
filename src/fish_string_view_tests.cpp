// These tests are adapted from string-view-t.cpp in string-view-lite.

// Copyright 2017-2020 Martin Moene
//
// https://github.com/martinmoene/string-view-lite
//
// Distributed under the Boost Software License, Version 1.0.
// (See accompanying file LICENSE.txt or copy at http://www.boost.org/LICENSE_1_0.txt)

#include "config.h"  // IWYU pragma: keep

#include <vector>

#include "fish_tests.h"
#include "nonstd/string_view.hpp"

using namespace nonstd;

namespace {
template <class T>
T* data(std::vector<T>& v) {
    return v.data();
}

void test_string_view_impl() {
    typedef string_view::size_type size_type;

    // 24.4.2.1 Construction and assignment:

    {
        string_view sv;

        // use parenthesis with data() to prevent lest from using sv.data() as C-string:

        do_test(sv.size() == size_type(0));
        do_test(sv.data() == nullptr);
    }

    {
        string_view sv("hello world", 5);

        do_test(sv.size() == size_type(5));
        do_test(*(sv.data() + 0) == 'h');
        do_test(*(sv.data() + 4) == 'o');
    }

    {
        string_view sv("hello world");

        do_test(sv.size() == size_type(11));
        do_test(*(sv.data() + 0) == 'h');
        do_test(*(sv.data() + 10) == 'd');
    }

    {
        string_view sv1;

        string_view sv2(sv1);

        // use parenthesis with data() to prevent lest from using sv.data() as C-string:

        do_test(sv2.size() == size_type(0));
        do_test(sv2.data() == nullptr);
    }

    {
        string_view sv1("hello world", 5);

        string_view sv2(sv1);

        do_test(sv2.size() == sv1.size());
        do_test(sv2.data() == sv1.data());
        do_test(*(sv2.data() + 0) == 'h');
        do_test(*(sv2.data() + 4) == 'o');
    }

    // Assignment:

    {
        string_view sv1;
        string_view sv2;

        sv2 = sv1;

        // use parenthesis with data() to prevent lest from using sv.data() as C-string:

        do_test(sv2.size() == size_type(0));
        do_test(sv2.data() == nullptr);
    }

    {
        string_view sv1("hello world", 5);
        string_view sv2;

        sv2 = sv1;

        // use parenthesis with data() to prevent lest from using sv.data() as C-string:

        do_test(sv2.size() == sv1.size());
        do_test(sv2.data() == sv1.data());
        do_test(*(sv2.data() + 0) == 'h');
        do_test(*(sv2.data() + 4) == 'o');
    }

    // 24.4.2.2 Iterator support:

    {
        char hello[] = "hello";
        string_view sv(hello);

        for (string_view::iterator pos = sv.begin(); pos != sv.end(); ++pos) {
            typedef std::iterator_traits<string_view::iterator>::difference_type difference_type;

            difference_type i = std::distance(sv.begin(), pos);
            do_test(*pos == *(sv.data() + i));
        }
    }

    {
        char hello[] = "hello";
        string_view sv(hello);

        for (string_view::const_iterator pos = sv.begin(); pos != sv.end(); ++pos) {
            typedef std::iterator_traits<string_view::const_iterator>::difference_type
                difference_type;

            difference_type i = std::distance(sv.cbegin(), pos);
            do_test(*pos == *(sv.data() + i));
        }
    }

    {
        char hello[] = "hello";
        string_view sv(hello);

        for (string_view::reverse_iterator pos = sv.rbegin(); pos != sv.rend(); ++pos) {
            typedef std::iterator_traits<string_view::reverse_iterator>::difference_type
                difference_type;

            difference_type dist = std::distance(sv.rbegin(), pos);
            do_test(*pos == *(sv.data() + sv.size() - 1 - dist));
        }
    }

    {
        char hello[] = "hello";
        string_view sv(hello);

        for (string_view::const_reverse_iterator pos = sv.crbegin(); pos != sv.crend(); ++pos) {
            typedef std::iterator_traits<string_view::const_reverse_iterator>::difference_type
                difference_type;

            difference_type dist = std::distance(sv.crbegin(), pos);
            do_test(*pos == *(sv.data() + sv.size() - 1 - dist));
        }
    }

    // 24.4.2.3 Capacity:

    {
        char hello[] = "hello";
        string_view sv(hello);

        do_test(sv.size() == std::char_traits<char>::length(hello));
    }

    {
        char hello[] = "hello";
        string_view sv(hello);

        do_test(sv.length() == std::char_traits<char>::length(hello));
    }

    {
        // "large"
        do_test(string_view().max_size() >=
                (std::numeric_limits<string_view::size_type>::max)() / 10);
    }

    {
        string_view sve;
        string_view svne("hello");

        do_test(sve.size() == size_type(0));
        do_test(sve.empty());
        do_test(!svne.empty());
    }

    // 24.4.2.4 Element access:

    {
        // Requires: index < sv.size()

        char hello[] = "hello";
        string_view sv(hello);

        for (size_type i = 0; i < sv.size(); ++i) {
            do_test(sv[i] == hello[i]);
        }
    }

    {
        char hello[] = "hello";
        string_view sv(hello);

        for (size_type i = 0; i < sv.size(); ++i) {
            do_test(sv.at(i) == hello[i]);
        }
    }

    {
        char hello[] = "hello";
        string_view sv(hello);

        do_test(*sv.data() == *sv.begin());

        for (size_type i = 0; i < sv.size(); ++i) {
            do_test(sv.data()[i] == hello[i]);
        }
    }

    {
        string_view sv;

        // use parenthesis with data() to prevent lest from using sv.data() as C-string:

        do_test(sv.data() == nullptr);
    }

    // 24.4.2.5 Modifiers:

    {
        char hello[] = "hello world";
        string_view sv(hello);

        sv.remove_prefix(6);

        do_test(sv.size() == size_type(5));
        do_test(std::equal(sv.begin(), sv.end(), hello + 6));
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        sv.remove_suffix(6);

        do_test(sv.size() == size_type(5));
        do_test(std::equal(sv.begin(), sv.end(), hello));
    }

    {
        char hello[] = "hello";
        char world[] = "world";
        string_view sv1(hello);
        string_view sv2(world);

        sv1.swap(sv2);

        do_test(std::equal(sv1.begin(), sv1.end(), world));
        do_test(std::equal(sv2.begin(), sv2.end(), hello));
    }

    // 24.4.2.6 String operations:

    {
        char hello[] = "hello world";
        string_view sv(hello);

        {
            std::vector<string_view::value_type> vec(sv.size());

            sv.copy(data(vec), vec.size());

            do_test(std::equal(vec.begin(), vec.end(), hello));
        }
        {
            std::size_t offset = 3u;
            std::size_t length = 4u;
            std::vector<string_view::value_type> vec(length);

            sv.copy(data(vec), length, offset);

            do_test(std::equal(vec.begin(), vec.end(), hello + offset));
        }
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        { do_test(std::equal(sv.begin(), sv.end(), sv.substr().begin())); }
        {
            string_view subv = sv.substr(6);

            do_test(std::equal(subv.begin(), subv.end(), hello + 6));
        }
        {
            string_view subv = sv.substr(3, 4);

            do_test(std::equal(subv.begin(), subv.end(), hello + 3));
        }
    }

    {
        char hello[] = "hello";
        char world[] = "world";

        do_test(string_view(hello).compare(string_view(hello)) == 0);
        do_test(string_view(hello).compare(string_view(world)) < 0);
        do_test(string_view(world).compare(string_view(hello)) > 0);

        char hello_sp[] = "hello ";

        do_test(string_view(hello).compare(string_view(hello_sp)) < 0);
        do_test(string_view(hello_sp).compare(string_view(hello)) > 0);
    }

    { do_test(string_view().compare(string_view()) == 0); }

    {
        string_view sv1("hello world");
        string_view sv2("world");

        do_test(sv1.compare(0, sv1.length(), sv1) == 0);
        do_test(sv1.compare(6, 5, sv2) == 0);
        do_test(sv1.compare(0, 5, sv2) < 0);
        do_test(sv2.compare(0, 5, sv1) > 0);
    }

    {
        string_view sv1("hello world");

        do_test(sv1.compare(0, sv1.length(), sv1) == 0);
        do_test(sv1.compare(6, 5, sv1, 6, 5) == 0);
        do_test(sv1.compare(0, 5, sv1, 6, 5) < 0);
        do_test(sv1.compare(6, 5, sv1, 0, 5) > 0);
    }

    {
        char hello[] = "hello";
        char world[] = "world";

        do_test(string_view(hello).compare(hello) == 0);
        do_test(string_view(hello).compare(world) < 0);
        do_test(string_view(world).compare(hello) > 0);
    }

    {
        char hello[] = "hello world";
        char world[] = "world";

        do_test(string_view(hello).compare(6, 5, world) == 0);
        do_test(string_view(hello).compare(world) < 0);
        do_test(string_view(world).compare(hello) > 0);
    }

    {
        char hello[] = "hello world";
        char world[] = "world";

        do_test(string_view(hello).compare(6, 5, world, 5) == 0);
        do_test(string_view(hello).compare(0, 5, world, 5) < 0);
        do_test(string_view(hello).compare(6, 5, hello, 5) > 0);
    }

    // 24.4.2.7 Searching:

    {
        char hello[] = "hello world";

        do_test(string_view(hello).starts_with(string_view(hello)));
        do_test(string_view(hello).starts_with(string_view("hello")));
        do_test(!string_view(hello).starts_with(string_view("world")));
    }

    {
        char hello[] = "hello world";

        do_test(string_view(hello).starts_with('h'));
        do_test(!string_view(hello).starts_with('e'));
    }

    {
        char hello[] = "hello world";

        do_test(string_view(hello).starts_with(hello));
        do_test(string_view(hello).starts_with("hello"));
        do_test(!string_view(hello).starts_with("world"));
    }

    {
        char hello[] = "hello world";

        do_test(string_view(hello).ends_with(string_view(hello)));
        do_test(string_view(hello).ends_with(string_view("world")));
        do_test(!string_view(hello).ends_with(string_view("hello")));
    }

    {
        char hello[] = "hello world";

        do_test(string_view(hello).ends_with('d'));
        do_test(!string_view(hello).ends_with('l'));
    }

    {
        char hello[] = "hello world";

        do_test(string_view(hello).ends_with(hello));
        do_test(string_view(hello).ends_with("world"));
        do_test(!string_view(hello).ends_with("hello"));
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find(sv) == size_type(0));
        do_test(sv.find(sv, 1) == string_view::npos);
        do_test(sv.find(string_view("world")) == size_type(6));
        do_test(sv.find(string_view("world"), 6) == size_type(6));
        do_test(sv.find(string_view("world"), 7) == string_view::npos);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find('h') == size_type(0));
        do_test(sv.find('h', 1) == string_view::npos);
        do_test(sv.find('w') == size_type(6));
        do_test(sv.find('w', 6) == size_type(6));
        do_test(sv.find('w', 7) == string_view::npos);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find(hello, 0, sv.size()) == size_type(0));
        do_test(sv.find(hello, 1, sv.size()) == string_view::npos);
        do_test(sv.find("world", 0, 5) == size_type(6));
        do_test(sv.find("world", 6, 5) == size_type(6));
        do_test(sv.find("world", 7, 4) == string_view::npos);
        do_test(sv.find("world", 3, 0) == size_type(3));
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find(hello) == size_type(0));
        do_test(sv.find(hello, 1) == string_view::npos);
        do_test(sv.find("world") == size_type(6));
        do_test(sv.find("world", 6) == size_type(6));
        do_test(sv.find("world", 7) == string_view::npos);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.rfind(sv) == size_type(0));
        do_test(sv.rfind(sv, 3) == size_type(0));
        do_test(sv.rfind(string_view()) == size_type(11));
        do_test(sv.rfind(string_view("world")) == size_type(6));
        do_test(sv.rfind(string_view("world"), 6) == size_type(6));
        do_test(sv.rfind(string_view("world"), 5) == string_view::npos);
        do_test(sv.rfind(string_view("hello world, a longer text")) == string_view::npos);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.rfind('h') == size_type(0));
        do_test(sv.rfind('e') == size_type(1));
        do_test(sv.rfind('e', 0) == string_view::npos);
        do_test(sv.rfind('w') == size_type(6));
        do_test(sv.rfind('w', 6) == size_type(6));
        do_test(sv.rfind('w', 5) == string_view::npos);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.rfind(hello) == size_type(0));
        do_test(sv.rfind(hello, 0, 5) == size_type(0));
        do_test(sv.rfind(hello, 1, 5) == size_type(0));
        do_test(sv.rfind("world", 10, 5) == size_type(6));
        do_test(sv.rfind("world", 6, 5) == size_type(6));
        do_test(sv.rfind("world", 5, 5) == string_view::npos);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.rfind(hello) == size_type(0));
        do_test(sv.rfind(hello, 3) == size_type(0));
        do_test(sv.rfind("world") == size_type(6));
        do_test(sv.rfind("world", 6) == size_type(6));
        do_test(sv.rfind("world", 5) == string_view::npos);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find_first_of(sv) == size_type(0));
        do_test(sv.find_first_of(sv, 3) == size_type(3));
        do_test(sv.find_first_of(string_view("xwo")) == size_type(4));
        do_test(sv.find_first_of(string_view("wdx"), 6) == size_type(6));
        do_test(sv.find_first_of(string_view("wxy"), 7) == string_view::npos);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find_first_of('h') == size_type(0));
        do_test(sv.find_first_of('h', 1) == string_view::npos);
        do_test(sv.find_first_of('w') == size_type(6));
        do_test(sv.find_first_of('w', 6) == size_type(6));
        do_test(sv.find_first_of('w', 7) == string_view::npos);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find_first_of(hello, 0, sv.size()) == size_type(0));
        do_test(sv.find_first_of(hello, 1, sv.size()) == size_type(1));
        do_test(sv.find_first_of("xwy", 0, 3) == size_type(6));
        do_test(sv.find_first_of("xwy", 6, 3) == size_type(6));
        do_test(sv.find_first_of("xwy", 7, 3) == string_view::npos);
        do_test(sv.find_first_of("xyw", 0, 2) == string_view::npos);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find_first_of(hello, 0) == size_type(0));
        do_test(sv.find_first_of(hello, 1) == size_type(1));
        do_test(sv.find_first_of("xwy", 0) == size_type(6));
        do_test(sv.find_first_of("xwy", 6) == size_type(6));
        do_test(sv.find_first_of("xwy", 7) == string_view::npos);
    }

    {
        char hello[] = "hello world";
        char empty[] = "";
        string_view sv(hello);
        string_view sve(empty);

        do_test(sv.find_last_of(sv) == size_type(10));
        do_test(sv.find_last_of(sv, 3) == size_type(3));
        do_test(sv.find_last_of(string_view("xwo")) == size_type(7));
        do_test(sv.find_last_of(string_view("wdx"), 6) == size_type(6));
        do_test(sv.find_last_of(string_view("wxy"), 7) == size_type(6));

        do_test(sve.find_last_of(string_view("x")) ==
                string_view::npos);  // issue 20 (endless loop)
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find_last_of('h') == size_type(0));
        do_test(sv.find_last_of('l', 1) == string_view::npos);
        do_test(sv.find_last_of('w') == size_type(6));
        do_test(sv.find_last_of('w', 6) == size_type(6));
        do_test(sv.find_last_of('w', 5) == string_view::npos);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find_last_of(hello, 0, sv.size()) == size_type(0));
        do_test(sv.find_last_of(hello, 1, sv.size()) == size_type(1));
        do_test(sv.find_last_of("xwy", 10, 3) == size_type(6));
        do_test(sv.find_last_of("xwy", 6, 3) == size_type(6));
        do_test(sv.find_last_of("xwy", 5, 3) == string_view::npos);
        do_test(sv.find_last_of("xyw", 10, 2) == string_view::npos);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find_last_of(hello, 0) == size_type(0));
        do_test(sv.find_last_of(hello, 1) == size_type(1));
        do_test(sv.find_last_of("xwy", 10) == size_type(6));
        do_test(sv.find_last_of("xwy", 6) == size_type(6));
        do_test(sv.find_last_of("xwy", 5) == string_view::npos);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find_first_not_of(sv) == string_view::npos);
        do_test(sv.find_first_not_of(sv, 3) == string_view::npos);
        do_test(sv.find_first_not_of(string_view("helo ")) == size_type(6));
        do_test(sv.find_first_not_of(string_view("helo "), 6) == size_type(6));
        do_test(sv.find_first_not_of(string_view("helo "), 7) == size_type(8));
        do_test(sv.find_first_not_of(string_view("helo wr")) == size_type(10));
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find_first_not_of('h') == size_type(1));
        do_test(sv.find_first_not_of('h', 1) == size_type(1));
        do_test(sv.find_first_not_of('w') == size_type(0));
        do_test(sv.find_first_not_of('w', 6) == size_type(7));
        do_test(sv.find_first_not_of('d', 10) == string_view::npos);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find_first_not_of(hello, 0, sv.size()) == string_view::npos);
        do_test(sv.find_first_not_of(hello, 3, sv.size()) == string_view::npos);
        do_test(sv.find_first_not_of("helo ", 0, 5) == size_type(6));
        do_test(sv.find_first_not_of("helo ", 6, 5) == size_type(6));
        do_test(sv.find_first_not_of("helo ", 7, 5) == size_type(8));
        do_test(sv.find_first_not_of("helo wr", 0, 7) == size_type(10));
        do_test(sv.find_first_not_of("he", 0, 1) == size_type(1));
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find_first_not_of(hello, 0) == string_view::npos);
        do_test(sv.find_first_not_of(hello, 3) == string_view::npos);
        do_test(sv.find_first_not_of("helo ", 0) == size_type(6));
        do_test(sv.find_first_not_of("helo ", 6) == size_type(6));
        do_test(sv.find_first_not_of("helo ", 7) == size_type(8));
        do_test(sv.find_first_not_of("helo wr", 0) == size_type(10));
    }

    {
        char hello[] = "hello world";
        char empty[] = "";
        string_view sv(hello);
        string_view sve(empty);

        do_test(sv.find_last_not_of(sv) == string_view::npos);
        do_test(sv.find_last_not_of(sv, 3) == string_view::npos);
        do_test(sv.find_last_not_of(string_view("world ")) == size_type(1));
        do_test(sv.find_last_not_of(string_view("heo "), 4) == size_type(3));
        do_test(sv.find_last_not_of(string_view("heo "), 3) == size_type(3));
        do_test(sv.find_last_not_of(string_view("heo "), 2) == size_type(2));
        do_test(sv.find_last_not_of(string_view("x")) == size_type(10));

        do_test(sve.find_last_not_of(string_view("x")) ==
                string_view::npos);  // issue 20 (endless loop)
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find_last_not_of('d') == size_type(9));
        do_test(sv.find_last_not_of('d', 10) == size_type(9));
        do_test(sv.find_last_not_of('d', 9) == size_type(9));
        do_test(sv.find_last_not_of('d', 8) == size_type(8));
        do_test(sv.find_last_not_of('d', 0) == size_type(0));
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find_last_not_of(hello, 0, sv.size()) == string_view::npos);
        do_test(sv.find_last_not_of(hello, 3, sv.size()) == string_view::npos);
        do_test(sv.find_last_not_of("world ", 10, 6) == size_type(1));
        do_test(sv.find_last_not_of("heo ", 4, 4) == size_type(3));
        do_test(sv.find_last_not_of("heo ", 3, 4) == size_type(3));
        do_test(sv.find_last_not_of("heo ", 2, 4) == size_type(2));
        do_test(sv.find_last_not_of("x") == size_type(10));
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        do_test(sv.find_last_not_of(hello, 0) == string_view::npos);
        do_test(sv.find_last_not_of(hello, 3) == string_view::npos);
        do_test(sv.find_last_not_of("world ", 10) == size_type(1));
        do_test(sv.find_last_not_of("heo ", 4) == size_type(3));
        do_test(sv.find_last_not_of("heo ", 3) == size_type(3));
        do_test(sv.find_last_not_of("heo ", 2) == size_type(2));
        do_test(sv.find_last_not_of("x") == size_type(10));
    }

    // 24.4.3 Non-member comparison functions:

    {
        char s[] = "hello";
        char t[] = "world";
        string_view sv(s);
        string_view tv(t);

        do_test(sv.length() == size_type(5));
        do_test(tv.length() == size_type(5));

        do_test(sv == sv);
        do_test(sv != tv);
        do_test(sv <= sv);
        do_test(sv <= tv);
        do_test(sv < tv);
        do_test(tv >= tv);
        do_test(tv >= sv);
        do_test(tv > sv);
    }

    {
        char s[] = "hello";
        string_view sv(s);

        do_test(sv == "hello");
        do_test("hello" == sv);

        do_test(sv != "world");
        do_test("world" != sv);

        do_test(sv < "world");
        do_test("aloha" < sv);

        do_test(sv <= "hello");
        do_test("hello" <= sv);
        do_test(sv <= "world");
        do_test("aloha" <= sv);

        do_test(sv > "aloha");
        do_test("world" > sv);

        do_test(sv >= "hello");
        do_test("hello" >= sv);
        do_test(sv >= "aloha");
        do_test("world" >= sv);
    }

    {
        string_view a, b;

        do_test(a == b);
        do_test(a.compare(b) == 0);
    }

    // 24.4.4 Inserters and extractors:

    // 24.4.5 Hash support (C++11):

    {
        do_test(std::hash<string_view>()("Hello, world!") ==
                std::hash<std::string>()("Hello, world!"));
    }

    {
        do_test(std::hash<wstring_view>()(L"Hello, world!") ==
                std::hash<std::wstring>()(L"Hello, world!"));
    }

    {
        do_test(std::hash<u32string_view>()(U"Hello, world!") ==
                std::hash<std::u32string>()(U"Hello, world!"));
    }

    // nonstd extension: conversions from and to std::basic_string

    {
        char hello[] = "hello world";
        std::string s = hello;

        string_view sv(hello);

        do_test(sv.size() == s.size());
        do_test(sv.compare(s) == 0);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        std::string s(sv);
        //  std::string t{ sv };

        do_test(sv.size() == s.size());
        do_test(sv.compare(s) == 0);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        std::string s1 = sv.to_string();

        do_test(sv.size() == s1.size());
        do_test(sv.compare(s1) == 0);

        std::string s2 = sv.to_string(std::string::allocator_type());

        do_test(sv.size() == s2.size());
        do_test(sv.compare(s2) == 0);
    }

    {
        char hello[] = "hello world";
        string_view sv(hello);

        std::string s1 = to_string(sv);

        do_test(sv.size() == s1.size());
        do_test(sv.compare(s1) == 0);

        std::string s2 = to_string(sv, std::string::allocator_type());

        do_test(sv.size() == s2.size());
        do_test(sv.compare(s2) == 0);
    }

    {
        char hello[] = "hello world";
        std::string s = hello;

        string_view sv = to_string_view(s);

        do_test(sv.size() == s.size());
        do_test(sv.compare(s) == 0);
    }
}
}  // namespace

void test_string_view() {
    say(L"Testing string view");
    test_string_view_impl();
}
