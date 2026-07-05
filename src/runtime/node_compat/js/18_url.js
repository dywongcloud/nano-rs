"use strict";
// node:url — legacy Url API + WHATWG re-exports (Node v22 semantics).
__nanoNodeRegister("url", function (module, exports, require) {
  const { makeError } = require("internal/errors");
  const punycode = require("punycode");
  const querystring = require("querystring");

  const protocolPattern = /^[a-z0-9.+-]+:/i;
  const portPattern = /:[0-9]*$/;
  const simplePathPattern = /^(\/\/?(?!\/)[^?\s]*)(\?[^\s]*)?$/;
  const unwise = ["{", "}", "|", "\\", "^", "`"];
  const autoEscape = ["'"].concat(unwise);
  const nonHostChars = ["%", "/", "?", ";", "#"].concat(autoEscape);
  const hostEndingChars = ["/", "?", "#"];
  const hostnameMaxLen = 255;
  const hostnamePartPattern = /^[+a-z0-9A-Z_-]{0,63}$/;
  const hostnamePartStart = /^([+a-z0-9A-Z_-]{0,63})(.*)$/;
  const unsafeProtocol = { javascript: true, "javascript:": true };
  const hostlessProtocol = { javascript: true, "javascript:": true };
  const slashedProtocol = {
    http: true, https: true, ftp: true, gopher: true, file: true, ws: true, wss: true,
    "http:": true, "https:": true, "ftp:": true, "gopher:": true, "file:": true, "ws:": true, "wss:": true,
  };

  function Url() {
    this.protocol = null;
    this.slashes = null;
    this.auth = null;
    this.host = null;
    this.port = null;
    this.hostname = null;
    this.hash = null;
    this.search = null;
    this.query = null;
    this.pathname = null;
    this.path = null;
    this.href = null;
  }

  Url.prototype.parse = function parse(url, parseQueryString, slashesDenoteHost) {
    if (typeof url !== "string") {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "url" argument must be of type string. Received ' + typeof url);
    }

    let hasHash = false;
    let start = -1;
    let end = -1;
    let rest = "";
    let lastPos = 0;
    for (let i = 0, inWs = false, split = false; i < url.length; ++i) {
      const code = url.charCodeAt(i);
      const isWs = code === 32 || code === 9 || code === 10 || code === 13 || code === 12 ||
        code === 0xfeff || code === 0xa0;
      if (start === -1) {
        if (isWs) continue;
        lastPos = start = i;
      } else if (inWs) {
        if (!isWs) {
          end = -1;
          inWs = false;
        }
      } else if (isWs) {
        end = i;
        inWs = true;
      }
      if (!split) {
        switch (code) {
          case 35: // '#'
            hasHash = true;
          // falls through
          case 63: // '?'
            split = true;
            break;
          case 92: // '\\'
            if (i - lastPos > 0) rest += url.slice(lastPos, i);
            rest += "/";
            lastPos = i + 1;
            break;
          default:
            break;
        }
      } else if (!hasHash && code === 35) {
        hasHash = true;
      }
    }

    if (start !== -1) {
      if (lastPos === start) {
        if (end === -1) {
          if (start === 0) rest = url;
          else rest = url.slice(start);
        } else {
          rest = url.slice(start, end);
        }
      } else if (end === -1 && lastPos < url.length) {
        rest += url.slice(lastPos);
      } else if (end !== -1 && lastPos < end) {
        rest += url.slice(lastPos, end);
      }
    }

    if (!slashesDenoteHost && !hasHash) {
      // Try fast path regexp
      const simplePath = simplePathPattern.exec(rest);
      if (simplePath) {
        this.path = rest;
        this.href = rest;
        this.pathname = simplePath[1];
        if (simplePath[2]) {
          this.search = simplePath[2];
          if (parseQueryString) {
            this.query = querystring.parse(this.search.slice(1));
          } else {
            this.query = this.search.slice(1);
          }
        } else if (parseQueryString) {
          this.search = null;
          this.query = Object.create(null);
        }
        return this;
      }
    }

    let proto = protocolPattern.exec(rest);
    let lowerProto;
    if (proto) {
      proto = proto[0];
      lowerProto = proto.toLowerCase();
      this.protocol = lowerProto;
      rest = rest.slice(proto.length);
    }

    let slashes;
    if (slashesDenoteHost || proto || /^\/\/[^@/]+@[^@/]+/.test(rest)) {
      slashes = rest.charCodeAt(0) === 47 && rest.charCodeAt(1) === 47;
      if (slashes && !(proto && hostlessProtocol[lowerProto])) {
        rest = rest.slice(2);
        this.slashes = true;
      }
    }

    if (!hostlessProtocol[lowerProto] && (slashes || (proto && !slashedProtocol[proto]))) {
      let hostEnd = -1;
      let atSign = -1;
      let nonHost = -1;
      for (let i = 0; i < rest.length; ++i) {
        switch (rest.charCodeAt(i)) {
          case 9: case 10: case 13: case 32: case 34: case 37: case 39: case 59:
          case 60: case 62: case 92: case 94: case 96: case 123: case 124: case 125:
            if (nonHost === -1) nonHost = i;
            break;
          case 35: case 47: case 63:
            if (nonHost === -1) nonHost = i;
            hostEnd = i;
            break;
          case 64:
            atSign = i;
            nonHost = -1;
            break;
          default:
            break;
        }
        if (hostEnd !== -1) break;
      }
      start = 0;
      if (atSign !== -1) {
        this.auth = decodeURIComponent(rest.slice(0, atSign));
        start = atSign + 1;
      }
      if (nonHost === -1) {
        this.host = rest.slice(start);
        rest = "";
      } else {
        this.host = rest.slice(start, nonHost);
        rest = rest.slice(nonHost);
      }

      this.parseHost();

      if (typeof this.hostname !== "string") this.hostname = "";

      const hostname = this.hostname;
      const ipv6Hostname = hostname.charCodeAt(0) === 91 &&
        hostname.charCodeAt(hostname.length - 1) === 93;

      if (!ipv6Hostname) {
        rest = getHostname(this, rest, hostname);
      }

      if (this.hostname.length > hostnameMaxLen) {
        this.hostname = "";
      } else {
        this.hostname = this.hostname.toLowerCase();
      }

      if (!ipv6Hostname) {
        this.hostname = punycode.toASCII(this.hostname);
      }

      const p = this.port ? ":" + this.port : "";
      const h = this.hostname || "";
      this.host = h + p;

      if (ipv6Hostname) {
        this.hostname = this.hostname.slice(1, -1);
        if (rest[0] !== "/") {
          rest = "/" + rest;
        }
      }
    }

    if (!unsafeProtocol[lowerProto]) {
      const escaped = autoEscapeStr(rest);
      if (escaped !== undefined) rest = escaped;
    }

    const hash = rest.indexOf("#");
    if (hash !== -1) {
      this.hash = rest.slice(hash);
      rest = rest.slice(0, hash);
    }
    const qm = rest.indexOf("?");
    if (qm !== -1) {
      this.search = rest.slice(qm);
      this.query = rest.slice(qm + 1);
      if (parseQueryString) {
        this.query = querystring.parse(this.query);
      }
      rest = rest.slice(0, qm);
    } else if (parseQueryString) {
      this.search = null;
      this.query = Object.create(null);
    }
    if (rest) this.pathname = rest;
    if (slashedProtocol[lowerProto] && this.hostname && !this.pathname) {
      this.pathname = "/";
    }

    if (this.pathname || this.search) {
      const p = this.pathname || "";
      const s = this.search || "";
      this.path = p + s;
    }

    this.href = this.format();
    return this;
  };

  // Percent-escape characters Node auto-escapes in the path portion.
  const escapedCodes = {
    9: "%09", 10: "%0A", 13: "%0D", 32: "%20", 34: "%22", 39: "%27",
    60: "%3C", 62: "%3E", 92: "%5C", 94: "%5E", 96: "%60",
    123: "%7B", 124: "%7C", 125: "%7D",
  };
  function autoEscapeStr(rest) {
    let escaped = "";
    let lastEscapedPos = 0;
    for (let i = 0; i < rest.length; ++i) {
      const escapedChar = escapedCodes[rest.charCodeAt(i)];
      if (escapedChar) {
        if (i > lastEscapedPos) {
          escaped += rest.slice(lastEscapedPos, i);
        }
        escaped += escapedChar;
        lastEscapedPos = i + 1;
      }
    }
    if (lastEscapedPos === 0) {
      return undefined;
    }
    if (lastEscapedPos < rest.length) {
      escaped += rest.slice(lastEscapedPos);
    }
    return escaped;
  }

  function getHostname(self, rest, hostname) {
    for (let i = 0; i < hostname.length; ++i) {
      const code = hostname.charCodeAt(i);
      const isValid = (code >= 97 && code <= 122) || code === 46 ||
        (code >= 65 && code <= 90) || (code >= 48 && code <= 57) ||
        code === 45 || code === 43 || code === 95 || code > 127;
      if (!isValid) {
        self.hostname = hostname.slice(0, i);
        return "/" + hostname.slice(i) + rest;
      }
    }
    return rest;
  }

  Url.prototype.parseHost = function parseHost() {
    let host = this.host;
    let port = portPattern.exec(host);
    if (port) {
      port = port[0];
      if (port !== ":") {
        this.port = port.slice(1);
      }
      host = host.slice(0, host.length - port.length);
    }
    if (host) this.hostname = host;
  };

  Url.prototype.format = function format() {
    let auth = this.auth || "";
    if (auth) {
      auth = encodeURIComponent(auth);
      auth = auth.replace(/%3A/i, ":");
      auth += "@";
    }

    let protocol = this.protocol || "";
    let pathname = this.pathname || "";
    let hash = this.hash || "";
    let host = "";
    let query = "";

    if (this.host) {
      host = auth + this.host;
    } else if (this.hostname) {
      host = auth + (this.hostname.includes(":") && !isIpv6Hostname(this.hostname)
        ? "[" + this.hostname + "]"
        : this.hostname);
      if (this.port) {
        host += ":" + this.port;
      }
    }

    if (this.query !== null && typeof this.query === "object" && Object.keys(this.query).length > 0) {
      query = querystring.stringify(this.query);
    }

    let search = this.search || (query && "?" + query) || "";

    if (protocol && protocol.charCodeAt(protocol.length - 1) !== 58) {
      protocol += ":";
    }

    let newPathname = "";
    let lastPos = 0;
    for (let i = 0; i < pathname.length; ++i) {
      switch (pathname.charCodeAt(i)) {
        case 35:
          if (i - lastPos > 0) newPathname += pathname.slice(lastPos, i);
          newPathname += "%23";
          lastPos = i + 1;
          break;
        case 63:
          if (i - lastPos > 0) newPathname += pathname.slice(lastPos, i);
          newPathname += "%3F";
          lastPos = i + 1;
          break;
        default:
          break;
      }
    }
    if (lastPos > 0) {
      if (lastPos !== pathname.length) pathname = newPathname + pathname.slice(lastPos);
      else pathname = newPathname;
    }

    if (this.slashes || slashedProtocol[protocol]) {
      if (this.slashes || host) {
        if (pathname && pathname.charCodeAt(0) !== 47) pathname = "/" + pathname;
        host = "//" + host;
      } else if (protocol.length >= 4 && protocol.slice(0, 4) === "file") {
        host = "//";
      }
    }

    search = search.replace(/#/g, "%23");

    if (hash && hash.charCodeAt(0) !== 35) hash = "#" + hash;
    if (search && search.charCodeAt(0) !== 63) search = "?" + search;

    return protocol + host + pathname + search + hash;
  };

  function isIpv6Hostname(hostname) {
    return hostname.charCodeAt(0) === 91 && hostname.charCodeAt(hostname.length - 1) === 93;
  }

  Url.prototype.resolve = function resolve(relative) {
    return this.resolveObject(urlParse(relative, false, true)).format();
  };

  Url.prototype.resolveObject = function resolveObject(relative) {
    if (typeof relative === "string") {
      const rel = new Url();
      rel.parse(relative, false, true);
      relative = rel;
    }

    const result = new Url();
    for (const key of Object.keys(this)) {
      result[key] = this[key];
    }

    result.hash = relative.hash;

    if (relative.href === "") {
      result.href = result.format();
      return result;
    }

    if (relative.slashes && !relative.protocol) {
      for (const key of Object.keys(relative)) {
        if (key !== "protocol") result[key] = relative[key];
      }
      if (slashedProtocol[result.protocol] && result.hostname && !result.pathname) {
        result.pathname = "/";
        result.path = result.pathname + (result.search || "");
      }
      result.href = result.format();
      return result;
    }

    if (relative.protocol && relative.protocol !== result.protocol) {
      if (!slashedProtocol[relative.protocol]) {
        for (const key of Object.keys(relative)) {
          result[key] = relative[key];
        }
        result.href = result.format();
        return result;
      }
      result.protocol = relative.protocol;
      if (!relative.host && !/^file:?$/.test(relative.protocol) && !hostlessProtocol[relative.protocol]) {
        const relPath = (relative.pathname || "").split("/");
        while (relPath.length && !(relative.host = relPath.shift()));
        if (!relative.host) relative.host = "";
        if (!relative.hostname) relative.hostname = "";
        if (relPath[0] !== "") relPath.unshift("");
        if (relPath.length < 2) relPath.unshift("");
        result.pathname = relPath.join("/");
      } else {
        result.pathname = relative.pathname;
      }
      result.search = relative.search;
      result.query = relative.query;
      result.host = relative.host || "";
      result.auth = relative.auth;
      result.hostname = relative.hostname || relative.host;
      result.port = relative.port;
      if (result.pathname || result.search) {
        const p = result.pathname || "";
        const s = result.search || "";
        result.path = p + s;
      }
      result.slashes = result.slashes || relative.slashes;
      result.href = result.format();
      return result;
    }

    const isSourceAbs = result.pathname && result.pathname.charAt(0) === "/";
    const isRelAbs = relative.host || (relative.pathname && relative.pathname.charAt(0) === "/");
    let mustEndAbs = isRelAbs || isSourceAbs || (result.host && relative.pathname);
    const removeAllDots = mustEndAbs;
    let srcPath = (result.pathname && result.pathname.split("/")) || [];
    const relPath = (relative.pathname && relative.pathname.split("/")) || [];
    const noLeadingSlashes = result.protocol && !slashedProtocol[result.protocol];

    if (noLeadingSlashes) {
      result.hostname = "";
      result.port = null;
      if (result.host) {
        if (srcPath[0] === "") srcPath[0] = result.host;
        else srcPath.unshift(result.host);
      }
      result.host = "";
      if (relative.protocol) {
        relative.hostname = null;
        relative.port = null;
        result.auth = null;
        if (relative.host) {
          if (relPath[0] === "") relPath[0] = relative.host;
          else relPath.unshift(relative.host);
        }
        relative.host = null;
      }
      mustEndAbs = mustEndAbs && (relPath[0] === "" || srcPath[0] === "");
    }

    if (isRelAbs) {
      if (relative.host || relative.host === "") {
        if (result.host !== relative.host) result.auth = null;
        result.host = relative.host;
        result.port = relative.port;
      }
      if (relative.hostname || relative.hostname === "") {
        if (result.hostname !== relative.hostname) result.auth = null;
        result.hostname = relative.hostname;
      }
      result.search = relative.search;
      result.query = relative.query;
      srcPath = relPath;
    } else if (relPath.length) {
      if (!srcPath) srcPath = [];
      srcPath.pop();
      srcPath = srcPath.concat(relPath);
      result.search = relative.search;
      result.query = relative.query;
    } else if (relative.search !== null && relative.search !== undefined) {
      if (noLeadingSlashes) {
        result.hostname = result.host = srcPath.shift();
        const authInHost = result.host && result.host.indexOf("@") > 0 && result.host.split("@");
        if (authInHost) {
          result.auth = authInHost.shift();
          result.host = result.hostname = authInHost.shift();
        }
      }
      result.search = relative.search;
      result.query = relative.query;
      if (result.pathname !== null || result.search !== null) {
        result.path = (result.pathname ? result.pathname : "") + (result.search ? result.search : "");
      }
      result.href = result.format();
      return result;
    }

    if (!srcPath.length) {
      result.pathname = null;
      if (result.search) {
        result.path = "/" + result.search;
      } else {
        result.path = null;
      }
      result.href = result.format();
      return result;
    }

    let last = srcPath.slice(-1)[0];
    const hasTrailingSlash =
      ((result.host || relative.host || srcPath.length > 1) && (last === "." || last === "..")) || last === "";

    let up = 0;
    for (let i = srcPath.length - 1; i >= 0; i--) {
      last = srcPath[i];
      if (last === ".") {
        srcPath.splice(i, 1);
      } else if (last === "..") {
        srcPath.splice(i, 1);
        up++;
      } else if (up) {
        srcPath.splice(i, 1);
        up--;
      }
    }

    if (!mustEndAbs && !removeAllDots) {
      while (up--) {
        srcPath.unshift("..");
      }
    }

    if (mustEndAbs && srcPath[0] !== "" && (!srcPath[0] || srcPath[0].charAt(0) !== "/")) {
      srcPath.unshift("");
    }

    if (hasTrailingSlash && srcPath.join("/").slice(-1) !== "/") {
      srcPath.push("");
    }

    const isAbsolute = srcPath[0] === "" || (srcPath[0] && srcPath[0].charAt(0) === "/");

    if (noLeadingSlashes) {
      result.hostname = result.host = isAbsolute ? "" : srcPath.length ? srcPath.shift() : "";
      const authInHost = result.host && result.host.indexOf("@") > 0 ? result.host.split("@") : false;
      if (authInHost) {
        result.auth = authInHost.shift();
        result.host = result.hostname = authInHost.shift();
      }
    }

    mustEndAbs = mustEndAbs || (result.host && srcPath.length);

    if (mustEndAbs && !isAbsolute) {
      srcPath.unshift("");
    }

    if (!srcPath.length) {
      result.pathname = null;
      result.path = null;
    } else {
      result.pathname = srcPath.join("/");
    }

    if (result.pathname !== null || result.search !== null) {
      result.path = (result.pathname ? result.pathname : "") + (result.search ? result.search : "");
    }
    result.auth = relative.auth || result.auth;
    result.slashes = result.slashes || relative.slashes;
    result.href = result.format();
    return result;
  };

  function urlParse(url, parseQueryString, slashesDenoteHost) {
    if (url instanceof Url) return url;
    const u = new Url();
    u.parse(url, parseQueryString, slashesDenoteHost);
    return u;
  }

  function urlFormat(urlObject, options) {
    if (typeof urlObject === "string") {
      urlObject = urlParse(urlObject);
    } else if (urlObject instanceof URL ||
        (urlObject !== null && typeof urlObject === "object" &&
         typeof urlObject.href === "string" && typeof urlObject.searchParams === "object" &&
         !(urlObject instanceof Url))) {
      // WHATWG URL with options
      let ret = "";
      const auth = options === undefined || options.auth !== false;
      const fragment = options === undefined || options.fragment !== false;
      const search = options === undefined || options.search !== false;
      const unicode = options !== undefined && options.unicode === true;
      ret += urlObject.protocol + "//";
      if (auth && urlObject.username) {
        ret += urlObject.username;
        if (urlObject.password) ret += ":" + urlObject.password;
        ret += "@";
      }
      ret += unicode ? punycode.toUnicode(urlObject.hostname) : urlObject.hostname;
      if (urlObject.port) ret += ":" + urlObject.port;
      ret += urlObject.pathname;
      if (search) ret += urlObject.search;
      if (fragment) ret += urlObject.hash;
      return ret;
    } else if (!(urlObject instanceof Url)) {
      return Url.prototype.format.call(urlObject);
    }
    return urlObject.format();
  }

  function urlResolve(source, relative) {
    return urlParse(source, false, true).resolve(relative);
  }

  function urlResolveObject(source, relative) {
    if (!source) return relative;
    return urlParse(source, false, true).resolveObject(relative);
  }

  function domainToASCII(domain) {
    try {
      return new URL("http://" + domain).hostname;
    } catch (_e) {
      try {
        return punycode.toASCII(String(domain).toLowerCase());
      } catch (_e2) {
        return "";
      }
    }
  }

  function domainToUnicode(domain) {
    return punycode.toUnicode(domainToASCII(domain) || String(domain));
  }

  function fileURLToPath(input, options) {
    let url = input;
    if (typeof input === "string") {
      url = new URL(input);
    } else if (!(input instanceof URL) &&
               !(input !== null && typeof input === "object" && typeof input.protocol === "string" && typeof input.pathname === "string")) {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "path" argument must be of type string or an instance of URL');
    }
    if (url.protocol !== "file:") {
      throw makeError(TypeError, "ERR_INVALID_URL_SCHEME", "The URL must be of scheme file");
    }
    if (url.hostname !== "" && url.hostname !== "localhost") {
      throw makeError(TypeError, "ERR_INVALID_FILE_URL_HOST", 'File URL host must be "localhost" or empty on linux');
    }
    const pathname = url.pathname;
    for (let n = 0; n < pathname.length; n++) {
      if (pathname[n] === "%") {
        const third = pathname.codePointAt(n + 2) | 0x20;
        if (pathname[n + 1] === "2" && third === 102) {
          throw makeError(TypeError, "ERR_INVALID_FILE_URL_PATH", "File URL path must not include encoded / characters");
        }
      }
    }
    return decodeURIComponent(pathname);
  }

  function pathToFileURL(filepath) {
    const outURL = new URL("file://");
    let resolved = require("path").resolve(String(filepath));
    const filePathLast = String(filepath).charCodeAt(String(filepath).length - 1);
    if (filePathLast === 47 && resolved[resolved.length - 1] !== "/") {
      resolved += "/";
    }
    outURL.pathname = resolved
      .replace(/%/g, "%25")
      .replace(/\\/g, "%5C")
      .replace(/\n/g, "%0A")
      .replace(/\r/g, "%0D")
      .replace(/\t/g, "%09")
      .replace(/#/g, "%23")
      .replace(/\?/g, "%3F");
    return outURL;
  }

  function urlToHttpOptions(url) {
    const options = {
      protocol: url.protocol,
      hostname: typeof url.hostname === "string" && url.hostname.startsWith("[")
        ? url.hostname.slice(1, -1)
        : url.hostname,
      hash: url.hash,
      search: url.search,
      pathname: url.pathname,
      path: (url.pathname || "") + (url.search || ""),
      href: url.href,
    };
    if (url.port !== "") {
      options.port = Number(url.port);
    }
    if (url.username || url.password) {
      options.auth = decodeURIComponent(url.username) + ":" + decodeURIComponent(url.password);
    }
    return options;
  }

  module.exports = {
    Url,
    parse: urlParse,
    resolve: urlResolve,
    resolveObject: urlResolveObject,
    format: urlFormat,
    URL: globalThis.URL,
    URLSearchParams: globalThis.URLSearchParams,
    domainToASCII,
    domainToUnicode,
    pathToFileURL,
    fileURLToPath,
    fileURLToPathBuffer(input, options) {
      const { Buffer } = require("buffer");
      return Buffer.from(fileURLToPath(input, options), "utf8");
    },
    urlToHttpOptions,
  };
});
