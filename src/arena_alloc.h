// An arena-allocator.
#ifndef FISH_ARENA_ALLOC_H
#define FISH_ARENA_ALLOC_H

#include "config.h"

#include <stdio.h>
#include <stdlib.h>

#include <cassert>
#include <cstddef>
#include <memory>
#include <vector>

/// This is a classic "bump pointer allocator" which allocates chunks of memory.
/// All such allocated objects may be cheaply deallocated.
/// Objects larger than the chunk are allocated via malloc.
/// Objects allocated here do not have their destructors run.
class arena_alloc_t {
   public:
    /// Create an arena with a suggested contents size.
    explicit arena_alloc_t(uint32_t contents_size) : contents_size_(contents_size) {}

    arena_alloc_t(const arena_alloc_t &) = delete;
    arena_alloc_t(arena_alloc_t &&) = delete;
    void operator=(const arena_alloc_t &) = delete;
    void operator=(arena_alloc_t &&) = delete;

    // Allocate raw memory for a 'T'.
    // Important: no constructors are invoked.
    template <typename T>
    void *alloc() {
        return allocN<T>(1);
    }

    // Allocate raw memory for N Ts.
    // Returns nullptr if N is 0.
    template <typename T>
    void *allocN(size_t N) {
        static_assert(std::is_trivially_destructible<T>::value,
                      "Object must not have a destructor");
        static_assert(std::is_trivially_copyable<T>::value, "Object must be trivially copyable");

        // Compute the length of the allocation in bytes, mindful of overflow.
        size_t length = N * sizeof(T);
        if (length / sizeof(T) != N) {
            // Overflow.
            abort();
        }
        if (length == 0) {
            return nullptr;
        }

        // Allocate a chunk if needed.
        if (!top_) push_chunk();
        assert(top_ && "Should have chunk");

        // Allocate from our chunk.
        void *ptr = try_bump_ptr_alloc(top_, alignof(T), length);
        if (!ptr && top_->remaining < contents_size_) {
            // We didn't fit, but our chunk was partially filled.
            // Try a new chunk.
            push_chunk();
            try_bump_ptr_alloc(top_, alignof(T), length);
        }
        if (!ptr) {
            // Still didn't fit, do a huge allocation.
            ptr = huge_alloc(length);
            assert(ptr && "huge_alloc should succeed or abort");
        }
        return ptr;
    }

    ~arena_alloc_t() {
        chunk_t *cursor = top_;
        while (cursor) {
            chunk_t *prev = cursor->prev;
            free(cursor);
            cursor = prev;
        }
        for (void *alloc : huge_allocs_) {
            free(alloc);
        }
    }

   private:
    // A chunk of memory. This is created via malloc().
    struct chunk_t {
        // Pointer to previous chunk.
        chunk_t *prev;

        // How much is remaining in this chunk.
        uint32_t remaining;

        // The bytes themselves.
        uint8_t contents[];
    };

    // Try allocating space in \p chunk with the given alignment and length.
    // Return a pointer, or nullptr on failure.
    void *try_bump_ptr_alloc(chunk_t *chunk, size_t align, size_t length) {
        assert(chunk && "Null chunk");
        assert(chunk->remaining <= contents_size_ && "Can't allocate more than size");
        size_t space = chunk->remaining;
        void *ptr = &chunk->contents[contents_size_ - space];
        if (!std::align(align, length, ptr, space)) {
            // Doesn't fit.
            return nullptr;
        }
        // Adjust remaining by the amount of alignment used.
        assert(space <= contents_size_ &&
               "Should not have adjusted space larger than contents size");
        chunk->remaining = static_cast<uint32_t>(space);

        // Now perform the allocation.
        assert(chunk->remaining >= length && "Should have enough space");
        chunk->remaining -= length;
        return ptr;
    }

    // Use calloc to allocate space for \p size bytes.
    // \return the pointer.
    void *huge_alloc(size_t size) {
        void *mem = calloc(1, size);
        if (!mem) {
            perror("calloc");
            abort();
        }
        huge_allocs_.push_back(mem);
        return mem;
    }

    // Allocate a new chunk, setting it as top.
    void push_chunk() {
        chunk_t *chunk = static_cast<chunk_t *>(calloc(1, contents_size_ + sizeof(chunk_t)));
        if (!chunk) {
            perror("calloc");
            abort();
        }
        chunk->remaining = contents_size_;
        chunk->prev = top_;
        top_ = chunk;
    }

    // The topmost chunk.
    chunk_t *top_{nullptr};

    // The size of each chunk.
    uint32_t contents_size_;

    // List of malloc-allocations larger than chunksize.
    std::vector<void *> huge_allocs_{};
};

#endif
