#ifndef __RCC_STDATOMIC_H
#define __RCC_STDATOMIC_H

#include <stddef.h>
#include <stdint.h>
#include <uchar.h>

/*
 * C11 atomics compatibility surface for hosted probes.
 *
 * rcc parses and preserves `_Atomic` types.  Load/store of atomic lvalues are
 * emitted as LLVM monotonic atomic memory operations when the LLVM backend is
 * enabled; the generic function-like helpers below keep real-world source
 * compatibility for projects such as QuickJS while fuller memory-order
 * lowering remains future compiler work.
 */

typedef _Atomic(_Bool) atomic_bool;
typedef _Atomic(char) atomic_char;
typedef _Atomic(signed char) atomic_schar;
typedef _Atomic(unsigned char) atomic_uchar;
typedef _Atomic(short) atomic_short;
typedef _Atomic(unsigned short) atomic_ushort;
typedef _Atomic(int) atomic_int;
typedef _Atomic(unsigned int) atomic_uint;
typedef _Atomic(long) atomic_long;
typedef _Atomic(unsigned long) atomic_ulong;
typedef _Atomic(long long) atomic_llong;
typedef _Atomic(unsigned long long) atomic_ullong;
typedef _Atomic(char16_t) atomic_char16_t;
typedef _Atomic(char32_t) atomic_char32_t;
typedef _Atomic(wchar_t) atomic_wchar_t;
typedef _Atomic(int_least8_t) atomic_int_least8_t;
typedef _Atomic(uint_least8_t) atomic_uint_least8_t;
typedef _Atomic(int_least16_t) atomic_int_least16_t;
typedef _Atomic(uint_least16_t) atomic_uint_least16_t;
typedef _Atomic(int_least32_t) atomic_int_least32_t;
typedef _Atomic(uint_least32_t) atomic_uint_least32_t;
typedef _Atomic(int_least64_t) atomic_int_least64_t;
typedef _Atomic(uint_least64_t) atomic_uint_least64_t;
typedef _Atomic(int_fast8_t) atomic_int_fast8_t;
typedef _Atomic(uint_fast8_t) atomic_uint_fast8_t;
typedef _Atomic(int_fast16_t) atomic_int_fast16_t;
typedef _Atomic(uint_fast16_t) atomic_uint_fast16_t;
typedef _Atomic(int_fast32_t) atomic_int_fast32_t;
typedef _Atomic(uint_fast32_t) atomic_uint_fast32_t;
typedef _Atomic(int_fast64_t) atomic_int_fast64_t;
typedef _Atomic(uint_fast64_t) atomic_uint_fast64_t;
typedef _Atomic(intptr_t) atomic_intptr_t;
typedef _Atomic(uintptr_t) atomic_uintptr_t;
typedef _Atomic(size_t) atomic_size_t;
typedef _Atomic(ptrdiff_t) atomic_ptrdiff_t;
typedef _Atomic(intmax_t) atomic_intmax_t;
typedef _Atomic(uintmax_t) atomic_uintmax_t;

typedef struct {
    atomic_bool __flag;
} atomic_flag;

#define memory_order_relaxed 0
#define memory_order_consume 1
#define memory_order_acquire 2
#define memory_order_release 3
#define memory_order_acq_rel 4
#define memory_order_seq_cst 5
typedef int memory_order;

#define ATOMIC_BOOL_LOCK_FREE 2
#define ATOMIC_CHAR_LOCK_FREE 2
#define ATOMIC_CHAR16_T_LOCK_FREE 2
#define ATOMIC_CHAR32_T_LOCK_FREE 2
#define ATOMIC_WCHAR_T_LOCK_FREE 2
#define ATOMIC_SHORT_LOCK_FREE 2
#define ATOMIC_INT_LOCK_FREE 2
#define ATOMIC_LONG_LOCK_FREE 2
#define ATOMIC_LLONG_LOCK_FREE 2
#define ATOMIC_POINTER_LOCK_FREE 2

#define ATOMIC_VAR_INIT(value) (value)
#define ATOMIC_FLAG_INIT { 0 }

#define kill_dependency(value) (value)
#define atomic_is_lock_free(obj) ((void)(obj), 1)
#define atomic_init(ptr, val) ((void)(*(ptr) = (val)))
#define atomic_load(ptr) (*(ptr))
#define atomic_load_explicit(ptr, order) ((void)(order), atomic_load(ptr))
#define atomic_store(ptr, val) ((void)(*(ptr) = (val)))
#define atomic_store_explicit(ptr, val, order) ((void)(order), atomic_store((ptr), (val)))

#define atomic_exchange(ptr, val)                                                     \
    ({                                                                                \
        unsigned long long __rcc_atomic_old = *(ptr);                                 \
        *(ptr) = (val);                                                               \
        __rcc_atomic_old;                                                             \
    })
#define atomic_exchange_explicit(ptr, val, order) ((void)(order), atomic_exchange((ptr), (val)))

#define atomic_fetch_add(ptr, val)                                                    \
    ({                                                                                \
        unsigned long long __rcc_atomic_old = *(ptr);                                 \
        *(ptr) = __rcc_atomic_old + (val);                                            \
        __rcc_atomic_old;                                                             \
    })
#define atomic_fetch_sub(ptr, val)                                                    \
    ({                                                                                \
        unsigned long long __rcc_atomic_old = *(ptr);                                 \
        *(ptr) = __rcc_atomic_old - (val);                                            \
        __rcc_atomic_old;                                                             \
    })
#define atomic_fetch_or(ptr, val)                                                     \
    ({                                                                                \
        unsigned long long __rcc_atomic_old = *(ptr);                                 \
        *(ptr) = __rcc_atomic_old | (val);                                            \
        __rcc_atomic_old;                                                             \
    })
#define atomic_fetch_xor(ptr, val)                                                    \
    ({                                                                                \
        unsigned long long __rcc_atomic_old = *(ptr);                                 \
        *(ptr) = __rcc_atomic_old ^ (val);                                            \
        __rcc_atomic_old;                                                             \
    })
#define atomic_fetch_and(ptr, val)                                                    \
    ({                                                                                \
        unsigned long long __rcc_atomic_old = *(ptr);                                 \
        *(ptr) = __rcc_atomic_old & (val);                                            \
        __rcc_atomic_old;                                                             \
    })

#define atomic_fetch_add_explicit(ptr, val, order) ((void)(order), atomic_fetch_add((ptr), (val)))
#define atomic_fetch_sub_explicit(ptr, val, order) ((void)(order), atomic_fetch_sub((ptr), (val)))
#define atomic_fetch_or_explicit(ptr, val, order) ((void)(order), atomic_fetch_or((ptr), (val)))
#define atomic_fetch_xor_explicit(ptr, val, order) ((void)(order), atomic_fetch_xor((ptr), (val)))
#define atomic_fetch_and_explicit(ptr, val, order) ((void)(order), atomic_fetch_and((ptr), (val)))

#define atomic_compare_exchange_strong(ptr, expected, desired)                        \
    ({                                                                                \
        int __rcc_atomic_ok = (*(ptr) == *(expected));                                \
        if (__rcc_atomic_ok)                                                          \
            *(ptr) = (desired);                                                       \
        else                                                                          \
            *(expected) = *(ptr);                                                     \
        __rcc_atomic_ok;                                                              \
    })
#define atomic_compare_exchange_strong_explicit(ptr, expected, desired, success, failure) \
    ((void)(success), (void)(failure), atomic_compare_exchange_strong((ptr), (expected), (desired)))

#define atomic_thread_fence(order) ((void)(order))
#define atomic_signal_fence(order) ((void)(order))

extern _Bool atomic_flag_test_and_set(volatile atomic_flag *);
extern _Bool atomic_flag_test_and_set_explicit(volatile atomic_flag *, memory_order);
extern void atomic_flag_clear(volatile atomic_flag *);
extern void atomic_flag_clear_explicit(volatile atomic_flag *, memory_order);

#endif
