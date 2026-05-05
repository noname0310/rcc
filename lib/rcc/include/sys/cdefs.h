#ifndef __RCC_SYS_CDEFS_H
#define __RCC_SYS_CDEFS_H

/*
 * Minimal glibc sys/cdefs.h compatibility for hosted Linux parsing.
 *
 * These macros are declaration annotation shims only.  They deliberately do
 * not model glibc internals, ABI selection, fortified entry points, or C++
 * linkage.  Host libc provides the runtime implementation.
 */

#ifndef __BEGIN_DECLS
#define __BEGIN_DECLS
#endif

#ifndef __END_DECLS
#define __END_DECLS
#endif

#ifndef __THROW
#define __THROW
#endif

#ifndef __THROWNL
#define __THROWNL
#endif

#ifndef __NTH
#define __NTH(fct) fct
#endif

#ifndef __NTHNL
#define __NTHNL(fct) fct
#endif

#ifndef __P
#define __P(args) args
#endif

#ifndef __PMT
#define __PMT(args) args
#endif

#ifndef __nonnull
#define __nonnull(params)
#endif

#ifndef __attribute_nonnull__
#define __attribute_nonnull__(params)
#endif

#ifndef __wur
#define __wur
#endif

#ifndef __attribute_warn_unused_result__
#define __attribute_warn_unused_result__
#endif

#ifndef __attribute_malloc__
#define __attribute_malloc__
#endif

#ifndef __attribute_alloc_size__
#define __attribute_alloc_size__(params)
#endif

#ifndef __attribute_alloc_align__
#define __attribute_alloc_align__(params)
#endif

#ifndef __attribute_pure__
#define __attribute_pure__
#endif

#ifndef __attribute_const__
#define __attribute_const__
#endif

#ifndef __attribute_deprecated__
#define __attribute_deprecated__
#endif

#ifndef __attribute_deprecated_msg__
#define __attribute_deprecated_msg__(msg)
#endif

#ifndef __attribute_format_arg__
#define __attribute_format_arg__(x)
#endif

#ifndef __attribute_format_strfmon__
#define __attribute_format_strfmon__(a, b)
#endif

#ifndef __attribute_returns_twice__
#define __attribute_returns_twice__
#endif

#ifndef __attribute_maybe_unused__
#define __attribute_maybe_unused__
#endif

#ifndef __attribute_used__
#define __attribute_used__
#endif

#ifndef __attribute_noinline__
#define __attribute_noinline__
#endif

#ifndef __attribute_artificial__
#define __attribute_artificial__
#endif

#ifndef __attribute_copy__
#define __attribute_copy__(arg)
#endif

#ifndef __returns_nonnull
#define __returns_nonnull
#endif

#ifndef __attr_access
#define __attr_access(x)
#endif

#ifndef __fortified_attr_access
#define __fortified_attr_access(a, o, s)
#endif

#ifndef __attr_access_none
#define __attr_access_none(argno)
#endif

#ifndef __attr_dealloc
#define __attr_dealloc(dealloc, argno)
#endif

#ifndef __attr_dealloc_free
#define __attr_dealloc_free
#endif

#ifndef __restrict_arr
#define __restrict_arr restrict
#endif

#endif
