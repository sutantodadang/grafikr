#include <string.h>

#include <lua.h>
#include <lauxlib.h>
#include "sha2.h"

/* SHA2_VERSION comes from the rockspec (e.g. -DSHA2_VERSION=0.3.0) */
#ifndef SHA2_VERSION
    #error SHA2_VERSION is not defined
#endif

#define SHA2_TOSTRING_HELPER(x) #x
#define SHA2_TOSTRING(x) SHA2_TOSTRING_HELPER(x)
#define SHA2_VERSION_STRING SHA2_TOSTRING(SHA2_VERSION)

#if LUA_VERSION_NUM < 502
#  define lua_rawlen lua_objlen   /* used by carray_capi.h */
#endif

/*
 * carray objects (https://github.com/osch/lua-carray) are recognized through
 * the Carray C API: no link or rockspec dependency on the carray module.
 */
#define CARRAY_CAPI_IMPLEMENT_GET_CAPI 1
#if defined(__GNUC__)
/* carray_capi.h is kept verbatim; it forward-declares enum typedefs */
#  pragma GCC diagnostic push
#  pragma GCC diagnostic ignored "-Wpedantic"
#endif
#include "carray_capi.h"
#if defined(__GNUC__)
#  pragma GCC diagnostic pop
#endif

#define SHA2_DIGEST_CLASS_NAME	"sha2.digest"

// source: md5lib.c from md5 Lua library
/**
*  X-Or. Does a bit-a-bit exclusive-or of two strings.
*  @param s1: arbitrary binary string.
*  @param s2: arbitrary binary string with same length as s1.
*  @return  a binary string with same length as s1 and s2,
*   where each bit is the exclusive-or of the corresponding bits in s1-s2.
*/
static int ex_or (lua_State *L) {
	size_t l1, l2;
	const char *s1 = luaL_checklstring(L, 1, &l1);
	const char *s2 = luaL_checklstring(L, 2, &l2);
	luaL_Buffer b;
	luaL_argcheck( L, l1 == l2, 2, "lengths must be equal" );
	luaL_buffinit(L, &b);
	while (l1--) luaL_addchar(&b, (*s1++)^(*s2++));
	luaL_pushresult(&b);
	return 1;
}

/* ======================================================================== */

/*
 * Remembers the Carray C API of the last carray argument. Its
 * toReadableCarray() accepts any Lua value and returns NULL for everything
 * that is not one of its carrays, so it is tried first and the C API need not
 * be looked up in the metatable for each call. One per Lua state, an upvalue
 * of all functions that take data arguments (no static mutable state).
 */
typedef struct CapiCache {
	const carray_capi *capi;
} CapiCache;

#define CAPI_CACHE	lua_upvalueindex(1)

/* returns the carray at idx (and its C API in *capiOut), NULL if idx is no carray */
static const carray *to_carray(lua_State *L, int idx, const carray_capi **capiOut, carray_info *info) {
	CapiCache *cc = (CapiCache*)lua_touserdata(L, CAPI_CACHE);
	const carray_capi *capi = cc->capi;
	const carray *ca = NULL;

	if (lua_type(L, idx) != LUA_TUSERDATA) {
		return NULL;
	}
	ca = capi ? capi->toReadableCarray(L, idx, info) : NULL;
	if (!ca) {
		capi = carray_get_capi(L, idx, NULL);
		if (capi) {
			ca = capi->toReadableCarray(L, idx, info);
			if (ca) {
				cc->capi = capi;
			}
		}
	}
	*capiOut = ca ? capi : NULL;
	return ca;
}

/*
 * Data argument: a Lua string (or number, as luaL_checklstring) or a carray
 * with 1 byte elements, which is read directly from its buffer without
 * copying. No Lua code runs while the data is hashed, so the pointer stays
 * valid. Anything else raises the usual "string expected" argument error.
 */
static const uint8_t *check_data(lua_State *L, int arg, size_t *len) {
	if (lua_type(L, arg) == LUA_TUSERDATA) {
		const carray_capi *capi;
		carray_info info;
		const carray *ca = to_carray(L, arg, &capi, &info);
		if (ca) {
			const uint8_t *ptr;
			if (info.elementSize != 1) {
				luaL_argerror(L, arg, "carray elements must be 1 byte in size");
			}
			*len = info.elementCount;
			if (*len == 0) {
				return (const uint8_t*)"";
			}
			ptr = (const uint8_t*)capi->getReadableElementPtr(ca, 0, *len);
			if (!ptr) {
				luaL_error(L, "cannot access carray");
			}
			return ptr;
		}
	}
	return (const uint8_t*)luaL_checklstring(L, arg, len);
}

/* ======================================================================== */

/* hashing context of any of the three algorithms (SHA384_CTX is SHA512_CTX) */
typedef struct {
	int bits;
	union {
		SHA256_CTX c256;
		SHA512_CTX c512;
	} u;
} Digest;

static size_t digest_length(int bits) {
	switch (bits) {
		case 256: return SHA256_DIGEST_LENGTH;
		case 384: return SHA384_DIGEST_LENGTH;
		default:  return SHA512_DIGEST_LENGTH;
	}
}

static void digest_init(Digest *d) {
	switch (d->bits) {
		case 256: SHA256_Init(&d->u.c256); break;
		case 384: SHA384_Init(&d->u.c512); break;
		default:  SHA512_Init(&d->u.c512); break;
	}
}

static void digest_update(Digest *d, const uint8_t *data, size_t len) {
	switch (d->bits) {
		case 256: SHA256_Update(&d->u.c256, data, len); break;
		case 384: SHA384_Update(&d->u.c512, data, len); break;
		default:  SHA512_Update(&d->u.c512, data, len); break;
	}
}

/* writes the binary digest to out; the context is wiped (re-init to reuse) */
static void digest_final(Digest *d, uint8_t *out) {
	switch (d->bits) {
		case 256: SHA256_Final(out, &d->u.c256); break;
		case 384: SHA384_Final(out, &d->u.c512); break;
		default:  SHA512_Final(out, &d->u.c512); break;
	}
}

static void push_digest(lua_State *L, const uint8_t *digest, size_t len, int hex) {
	if (hex) {
		static const char hexdigits[] = "0123456789abcdef";
		char buf[SHA512_DIGEST_LENGTH * 2];
		size_t i;
		for (i = 0; i < len; i++) {
			buf[2*i]   = hexdigits[digest[i] >> 4];
			buf[2*i+1] = hexdigits[digest[i] & 0x0f];
		}
		lua_pushlstring(L, buf, 2 * len);
	} else {
		lua_pushlstring(L, (const char*)digest, len);
	}
}

/* ======================================================================== */

/* one-shot hash of the data argument 1 */
static int oneshot(lua_State *L, int bits, int hex) {
	size_t len;
	const uint8_t *data = check_data(L, 1, &len);
	Digest d;
	uint8_t digest[SHA512_DIGEST_LENGTH];

	d.bits = bits;
	digest_init(&d);
	digest_update(&d, data, len);
	digest_final(&d, digest);
	push_digest(L, digest, digest_length(bits), hex);
	return 1;
}

/**
*  @param: an arbitrary binary string or byte carray.
*  @return: an SHA-256 hash string.
*/
static int sha256(lua_State *L) { return oneshot(L, 256, 0); }

/**
*  @param: an arbitrary binary string or byte carray.
*  @return: an SHA-256 hash as lowercase hex string.
*/
static int sha256hex(lua_State *L) { return oneshot(L, 256, 1); }

/**
*  @param: an arbitrary binary string or byte carray.
*  @return: an SHA-384 hash string.
*/
static int sha384(lua_State *L) { return oneshot(L, 384, 0); }

/**
*  @param: an arbitrary binary string or byte carray.
*  @return: an SHA-384 hash as lowercase hex string.
*/
static int sha384hex(lua_State *L) { return oneshot(L, 384, 1); }

/**
*  @param: an arbitrary binary string or byte carray.
*  @return: an SHA-512 hash string.
*/
static int sha512(lua_State *L) { return oneshot(L, 512, 0); }

/**
*  @param: an arbitrary binary string or byte carray.
*  @return: an SHA-512 hash as lowercase hex string.
*/
static int sha512hex(lua_State *L) { return oneshot(L, 512, 1); }

/* ======================================================================== */
/* streaming digest objects                                                 */

static Digest *check_digest(lua_State *L, int idx) {
	return (Digest*)luaL_checkudata(L, idx, SHA2_DIGEST_CLASS_NAME);
}

static Digest *new_digest(lua_State *L, int bits) {
	Digest *d = (Digest*)lua_newuserdata(L, sizeof(Digest));
	memset(d, 0, sizeof(Digest));
	d->bits = bits;
	luaL_getmetatable(L, SHA2_DIGEST_CLASS_NAME);
	lua_setmetatable(L, -2);
	return d;
}

/**
*  sha2.new(bits) -> digest object in its initial state
*  @param bits: 256, 384 or 512.
*/
static int digest_new(lua_State *L) {
	lua_Integer bits = luaL_checkinteger(L, 1);
	Digest *d;
	luaL_argcheck(L, bits == 256 || bits == 384 || bits == 512, 1,
	              "bits must be 256, 384 or 512");
	d = new_digest(L, (int)bits);
	digest_init(d);
	return 1;
}

/**
*  obj:add(data, ...) -> obj
*  Feeds the data arguments (strings or byte carrays) in order.
*/
static int digest_add(lua_State *L) {
	Digest *d = check_digest(L, 1);
	int i, n = lua_gettop(L);
	for (i = 2; i <= n; i++) {
		size_t len;
		const uint8_t *data = check_data(L, i, &len);
		digest_update(d, data, len);
	}
	lua_settop(L, 1);
	return 1;
}

/* finishes the hash, pushes the digest and resets the object */
static int digest_finish(lua_State *L, int hex) {
	Digest *d = check_digest(L, 1);
	uint8_t digest[SHA512_DIGEST_LENGTH];
	digest_final(d, digest);
	digest_init(d);
	push_digest(L, digest, digest_length(d->bits), hex);
	return 1;
}

/**
*  obj:digest() -> binary digest; the object is reset afterwards.
*/
static int digest_digest(lua_State *L) { return digest_finish(L, 0); }

/**
*  obj:hexdigest() -> lowercase hex digest; the object is reset afterwards.
*/
static int digest_hexdigest(lua_State *L) { return digest_finish(L, 1); }

/**
*  obj:reset() -> obj, back in the initial state.
*/
static int digest_reset(lua_State *L) {
	Digest *d = check_digest(L, 1);
	digest_init(d);
	lua_settop(L, 1);
	return 1;
}

/**
*  obj:clone() -> independent copy of obj with the current state.
*/
static int digest_clone(lua_State *L) {
	Digest *d = check_digest(L, 1);
	Digest *c = new_digest(L, d->bits);
	memcpy(c, d, sizeof(Digest));
	return 1;
}

/**
*  obj:algorithm() -> 256, 384 or 512
*/
static int digest_algorithm(lua_State *L) {
	Digest *d = check_digest(L, 1);
	lua_pushinteger(L, d->bits);
	return 1;
}

static int digest_tostring(lua_State *L) {
	Digest *d = check_digest(L, 1);
	lua_pushfstring(L, "%s (SHA-%d): %p", SHA2_DIGEST_CLASS_NAME, d->bits, (void*)d);
	return 1;
}

/* ======================================================================== */

static void set_info (lua_State *L) {
	lua_pushliteral (L, "_VERSION");
	lua_pushliteral (L, "sha2 " SHA2_VERSION_STRING);
	lua_settable (L, -3);
}

typedef struct {
	const char *name;
	lua_CFunction func;
} sha2_reg;

/* module functions, each gets the CapiCache as upvalue */
static const sha2_reg reg[] = {
	{"exor", ex_or},
	{"sha256", sha256},
	{"sha256hex", sha256hex},
	{"sha384", sha384},
	{"sha384hex", sha384hex},
	{"sha512", sha512},
	{"sha512hex", sha512hex},
	{"new", digest_new},
	{NULL, NULL}
};

/* methods of digest objects, each gets the CapiCache as upvalue */
static const sha2_reg methods[] = {
	{"add", digest_add},
	{"digest", digest_digest},
	{"hexdigest", digest_hexdigest},
	{"reset", digest_reset},
	{"clone", digest_clone},
	{"algorithm", digest_algorithm},
	{NULL, NULL}
};

/* sets the functions of r into the table at the top, with upvalue cc (an absolute index) */
static void set_funcs(lua_State *L, const sha2_reg *r, int cc) {
	for (; r->name; r++) {
		lua_pushstring(L, r->name);
		lua_pushvalue(L, cc);
		lua_pushcclosure(L, r->func, 1);
		lua_rawset(L, -3);
	}
}

int luaopen_sha2(lua_State *L) {
	int n = 0, cc;
	CapiCache *cache = (CapiCache*)lua_newuserdata(L, sizeof(CapiCache));
	cache->capi = NULL;
	cc = lua_gettop(L);

	if (luaL_newmetatable(L, SHA2_DIGEST_CLASS_NAME)) {
		lua_pushliteral(L, SHA2_DIGEST_CLASS_NAME);
		lua_setfield(L, -2, "__name");
		lua_pushcfunction(L, digest_tostring);
		lua_setfield(L, -2, "__tostring");
		lua_newtable(L);
		set_funcs(L, methods, cc);
		lua_setfield(L, -2, "__index");
	}
	lua_pop(L, 1);

	while (reg[n].name) n++;
	lua_createtable(L, 0, n + 1);
	set_funcs(L, reg, cc);
	set_info (L);
	lua_remove(L, cc);
#if LUA_VERSION_NUM < 502
	lua_pushvalue(L, -1);
	lua_setglobal(L, "sha2");
#endif
	return 1;
}
