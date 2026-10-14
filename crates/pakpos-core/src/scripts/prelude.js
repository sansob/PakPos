// PakPos script sandbox prelude: a Postman-compatible subset of the `pm` API.
// Input arrives as JSON in __PAKPOS_INPUT; everything the script changes is
// collected in __out and read back by Rust as JSON. There is no file, process
// or network access in this context.
"use strict";
const __in = JSON.parse(__PAKPOS_INPUT);
const __out = { tests: [], logs: [], env: {}, coll: {}, glob: {}, vars: {}, request: null, error: null };

const __fmt = (v) => {
  if (typeof v === "string") return v;
  try { return JSON.stringify(v); } catch (_) { return String(v); }
};
const __log = (level) => (...args) => { __out.logs.push((level ? level + " " : "") + args.map(__fmt).join(" ")); };
globalThis.console = { log: __log(""), info: __log(""), debug: __log(""), warn: __log("[warn]"), error: __log("[error]") };

const __str = (v) => (v === undefined || v === null ? v : typeof v === "object" ? JSON.stringify(v) : String(v));

function __store(base, out) {
  return {
    get(k) { if (k in out) return out[k] === null ? undefined : out[k]; return base[k]; },
    set(k, v) { out[k] = v === undefined || v === null ? "" : __str(v); },
    unset(k) { out[k] = null; },
    has(k) { return (k in out ? out[k] !== null : k in base); },
    clear() { for (const k of Object.keys(base)) out[k] = null; for (const k of Object.keys(out)) out[k] = null; },
    toObject() {
      const o = Object.assign({}, base);
      for (const [k, v] of Object.entries(out)) { if (v === null) delete o[k]; else o[k] = v; }
      return o;
    },
  };
}

const __env = __store(__in.environment, __out.env);
const __coll = __store(__in.collection, __out.coll);
const __glob = __store(__in.globals, __out.glob);
const __vars = __store(__in.variables, __out.vars);
// Local values first, then environment, collection and globals.
const __varsGet = __vars.get;
const __lookup = (k) => {
  for (const get of [__varsGet, __env.get, __coll.get, __glob.get]) { const v = get(k); if (v !== undefined) return v; }
  return undefined;
};
const __replaceIn = (text) => String(text).replace(/\{\{\s*([^{}]+?)\s*\}\}/g, (m, k) => { const v = __lookup(k); return v === undefined ? m : v; });
for (const s of [__env, __coll, __glob]) s.replaceIn = __replaceIn;
__vars.replaceIn = __replaceIn;
__vars.get = __lookup;

// ---- assertions (chai-style subset) ------------------------------------
class AssertionError extends Error { constructor(m) { super(m); this.name = "AssertionError"; } }
const __typeOf = (v) => (v === null ? "null" : Array.isArray(v) ? "array" : typeof v);
function __deepEqual(a, b) {
  if (a === b) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return Number.isNaN(a) && Number.isNaN(b);
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const ka = Object.keys(a), kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  return ka.every((k) => __deepEqual(a[k], b[k]));
}
function expect(actual, message) {
  const st = { neg: false, deep: false };
  const show = (v) => { try { return JSON.stringify(v); } catch (_) { return String(v); } };
  const check = (ok, what) => {
    if (ok === st.neg) throw new AssertionError((message ? message + ": " : "") + "expected " + show(actual) + (st.neg ? " not " : " ") + what);
    return api;
  };
  const api = {};
  for (const w of ["to", "be", "been", "is", "that", "which", "and", "has", "have", "with", "at", "of", "same", "does", "still"]) {
    Object.defineProperty(api, w, { get: () => api });
  }
  Object.defineProperty(api, "not", { get: () => { st.neg = !st.neg; return api; } });
  Object.defineProperty(api, "deep", { get: () => { st.deep = true; return api; } });
  const eq = (v) => check(st.deep ? __deepEqual(actual, v) : actual === v, "to equal " + show(v));
  api.equal = api.equals = api.eq = eq;
  api.eql = (v) => check(__deepEqual(actual, v), "to deeply equal " + show(v));
  api.above = api.gt = api.greaterThan = (n) => check(actual > n, "to be above " + n);
  api.below = api.lt = api.lessThan = (n) => check(actual < n, "to be below " + n);
  api.least = api.gte = (n) => check(actual >= n, "to be at least " + n);
  api.most = api.lte = (n) => check(actual <= n, "to be at most " + n);
  api.within = (a, b) => check(actual >= a && actual <= b, "to be within " + a + ".." + b);
  api.oneOf = (list) => check(list.some((x) => __deepEqual(x, actual)), "to be one of " + show(list));
  api.match = (re) => check(re.test(String(actual)), "to match " + re);
  const include = (v) => {
    let ok;
    if (typeof actual === "string") ok = actual.includes(v);
    else if (Array.isArray(actual)) ok = actual.some((x) => __deepEqual(x, v));
    else if (actual && typeof actual === "object") ok = typeof v === "object" ? Object.entries(v).every(([k, x]) => __deepEqual(actual[k], x)) : v in actual;
    else ok = false;
    return check(ok, "to include " + show(v));
  };
  api.include = api.includes = api.contain = api.contains = include;
  api.a = api.an = (t) => check(__typeOf(actual) === String(t).toLowerCase(), "to be a " + t);
  api.property = (name, ...val) => {
    const has = actual !== null && actual !== undefined && Object.prototype.hasOwnProperty.call(Object(actual), name);
    if (val.length === 0) return check(has, "to have property " + show(name));
    return check(has && __deepEqual(actual[name], val[0]), "to have property " + show(name) + " = " + show(val[0]));
  };
  api.lengthOf = api.length = (n) => check(actual != null && actual.length === n, "to have length " + n);
  api.keys = (...ks) => { const list = Array.isArray(ks[0]) ? ks[0] : ks; return check(actual && list.every((k) => k in actual), "to have keys " + show(list)); };
  for (const [name, pred, what] of [
    ["ok", (v) => !!v, "to be truthy"], ["true", (v) => v === true, "to be true"], ["false", (v) => v === false, "to be false"],
    ["null", (v) => v === null, "to be null"], ["undefined", (v) => v === undefined, "to be undefined"],
    ["exist", (v) => v !== null && v !== undefined, "to exist"], ["NaN", (v) => Number.isNaN(v), "to be NaN"],
    ["empty", (v) => (v == null ? true : typeof v === "object" && !Array.isArray(v) ? Object.keys(v).length === 0 : v.length === 0), "to be empty"],
  ]) {
    Object.defineProperty(api, name, { get: () => check(pred(actual), what) });
  }
  return api;
}

// ---- request / response -------------------------------------------------
function __headerList(list) {
  const find = (k) => list.findIndex((h) => h.key.toLowerCase() === String(k).toLowerCase());
  return {
    get: (k) => { const i = find(k); return i < 0 ? undefined : list[i].value; },
    has: (k, v) => { const i = find(k); return i >= 0 && (v === undefined || list[i].value === v); },
    add: (h) => { list.push({ key: String(h.key), value: __str(h.value) }); },
    upsert: (h) => { const i = find(h.key); if (i < 0) list.push({ key: String(h.key), value: __str(h.value) }); else list[i].value = __str(h.value); },
    remove: (k) => { let i; while ((i = find(k)) >= 0) list.splice(i, 1); },
    toObject: () => Object.fromEntries(list.map((h) => [h.key, h.value])),
    all: () => list.map((h) => ({ key: h.key, value: h.value })),
    each: (fn) => list.forEach((h) => fn({ key: h.key, value: h.value })),
    count: () => list.length,
  };
}

const __req = __in.request;
__out.request = __req;
const request = {
  get url() { return __req.url; },
  set url(v) { __req.url = String(v); },
  get method() { return __req.method; },
  set method(v) { __req.method = String(v).toUpperCase(); },
  headers: __headerList(__req.headers),
  body: {
    get raw() { return __req.body; },
    set raw(v) { __req.body = v === null || v === undefined ? null : String(v); },
  },
};

let response;
if (__in.response) {
  const r = __in.response;
  const headers = __headerList(r.headers);
  response = {
    code: r.code,
    status: r.status,
    headers,
    responseTime: r.time,
    responseSize: r.size,
    text: () => r.body,
    json: () => JSON.parse(r.body),
  };
  response.to = {
    have: {
      status: (code) => {
        if (typeof code === "number" ? r.code !== code : r.status !== code) throw new AssertionError("expected response to have status " + code + " but got " + r.code);
      },
      header: (k, v) => { if (!headers.has(k, v)) throw new AssertionError("expected response to have header " + k + (v === undefined ? "" : " = " + v)); },
      body: (t) => { if (t !== undefined && r.body !== t && !String(r.body).includes(t)) throw new AssertionError("expected body to contain " + t); },
      jsonBody: (path) => {
        let v; try { v = JSON.parse(r.body); } catch (e) { throw new AssertionError("expected a JSON body"); }
        if (path !== undefined && String(path).split(".").reduce((o, k) => (o == null ? undefined : o[k]), v) === undefined) throw new AssertionError("expected JSON body to have " + path);
      },
    },
    be: {},
  };
  for (const [name, pred] of [["ok", (c) => c >= 200 && c < 300], ["success", (c) => c >= 200 && c < 300], ["clientError", (c) => c >= 400 && c < 500], ["serverError", (c) => c >= 500], ["notFound", (c) => c === 404], ["unauthorized", (c) => c === 401], ["forbidden", (c) => c === 403]]) {
    Object.defineProperty(response.to.be, name, { get: () => { if (!pred(r.code)) throw new AssertionError("expected response to be " + name + " but got " + r.code); return true; } });
  }
  globalThis.responseBody = r.body;
  globalThis.responseCode = { code: r.code, name: r.status };
  globalThis.responseTime = r.time;
  globalThis.responseHeaders = headers.toObject();
}

const pm = {
  info: { requestName: __in.info.requestName, eventName: __in.info.eventName, iteration: 0 },
  environment: __env,
  collectionVariables: __coll,
  globals: __glob,
  variables: __vars,
  request,
  response,
  expect,
  test(name, fn) {
    try {
      fn();
      __out.tests.push({ name: String(name), passed: true, message: "passed" });
    } catch (e) {
      __out.tests.push({ name: String(name), passed: false, message: String((e && e.message) || e) });
    }
  },
  sendRequest() { throw new Error("pm.sendRequest is not available in the PakPos sandbox (no network access). Use a separate request with Extract instead."); },
};
globalThis.pm = pm;
globalThis.expect = expect;

// Legacy Postman globals.
globalThis.tests = {};
globalThis.postman = {
  setEnvironmentVariable: (k, v) => __env.set(k, v),
  getEnvironmentVariable: (k) => __env.get(k),
  clearEnvironmentVariable: (k) => __env.unset(k),
  setGlobalVariable: (k, v) => __glob.set(k, v),
  getGlobalVariable: (k) => __glob.get(k),
  clearGlobalVariable: (k) => __glob.unset(k),
};
globalThis.environment = __env.toObject();
globalThis.globals = __glob.toObject();
