#ifndef __RCC_STDATOMIC_H
#define __RCC_STDATOMIC_H

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

#endif
