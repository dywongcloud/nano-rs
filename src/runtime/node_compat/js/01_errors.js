"use strict";
// internal/errors — Node-shaped coded errors (CONTRACT.md §3).
__nanoNodeRegister("internal/errors", function (module, exports, require) {
  const codes = Object.create(null);

  // errno values follow Linux/libuv conventions.
  const UV_ERRNOS = {
    EPERM: -1, ENOENT: -2, EIO: -5, EBADF: -9, EAGAIN: -11, EACCES: -13,
    EBUSY: -16, EEXIST: -17, EXDEV: -18, ENODEV: -19, ENOTDIR: -20,
    EISDIR: -21, EINVAL: -22, EMFILE: -24, ENOSPC: -28, EPIPE: -32,
    ENOSYS: -38, ENOTEMPTY: -39, ENOTSOCK: -88, EMSGSIZE: -90,
    ECONNRESET: -104, ECONNREFUSED: -111, EADDRINUSE: -98,
    EADDRNOTAVAIL: -99, ENETUNREACH: -101, ETIMEDOUT: -110,
    EAI_AGAIN: -3001, ENOTFOUND: -3008, ECANCELED: -125,
  };

  function defineCode(code, Base, defaultMessage) {
    class NodeError extends Base {
      constructor(message) {
        super(message === undefined ? defaultMessage : message);
        this.code = code;
      }
      get name() {
        return Base.name + " [" + code + "]";
      }
    }
    Object.defineProperty(NodeError, "name", { value: code });
    codes[code] = NodeError;
    return NodeError;
  }

  function makeError(Base, code, message) {
    const err = new Base(message);
    err.code = code;
    return err;
  }

  function uvError(code, syscall, path) {
    const errno = UV_ERRNOS[code] !== undefined ? UV_ERRNOS[code] : -22;
    let message = code + ": " + syscall;
    if (path !== undefined && path !== null) {
      message += " '" + path + "'";
    }
    const err = new Error(message);
    err.code = code;
    err.errno = errno;
    err.syscall = syscall;
    if (path !== undefined && path !== null) {
      err.path = path;
    }
    return err;
  }

  defineCode("ERR_INVALID_ARG_TYPE", TypeError, "invalid argument type");
  defineCode("ERR_INVALID_ARG_VALUE", TypeError, "invalid argument value");
  defineCode("ERR_OUT_OF_RANGE", RangeError, "value out of range");
  defineCode("ERR_INVALID_CALLBACK", TypeError, "callback must be a function");
  defineCode("ERR_MISSING_ARGS", TypeError, "missing required arguments");
  defineCode("ERR_METHOD_NOT_IMPLEMENTED", Error, "method not implemented");
  defineCode("ERR_STREAM_DESTROYED", Error, "Cannot call write after a stream was destroyed");
  defineCode("ERR_STREAM_WRITE_AFTER_END", Error, "write after end");
  defineCode("ERR_STREAM_ALREADY_FINISHED", Error, "stream already finished");
  defineCode("ERR_STREAM_PREMATURE_CLOSE", Error, "Premature close");
  defineCode("ERR_STREAM_PUSH_AFTER_EOF", Error, "stream.push() after EOF");
  defineCode("ERR_STREAM_NULL_VALUES", TypeError, "May not write null values to stream");
  defineCode("ERR_STREAM_CANNOT_PIPE", Error, "Cannot pipe, not readable");
  defineCode("ERR_STREAM_UNSHIFT_AFTER_END_EVENT", Error, "stream.unshift() after end event");
  defineCode("ERR_UNHANDLED_ERROR", Error, "Unhandled error");
  defineCode("ERR_BUFFER_OUT_OF_BOUNDS", RangeError, "Attempt to access memory outside buffer bounds");
  defineCode("ERR_UNKNOWN_ENCODING", TypeError, "Unknown encoding");
  defineCode("ERR_INVALID_BUFFER_SIZE", RangeError, "Invalid buffer size");
  defineCode("ERR_CRYPTO_INVALID_DIGEST", TypeError, "Invalid digest");
  defineCode("ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH", RangeError, "Input buffers must have the same byte length");
  defineCode("ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE", TypeError, "Invalid key object type");
  defineCode("ERR_CRYPTO_INVALID_STATE", Error, "Invalid crypto state");
  defineCode("ERR_CRYPTO_ECDH_INVALID_PUBLIC_KEY", Error, "Public key is not valid for specified curve");
  defineCode("ERR_OPERATION_NOT_PERMITTED", Error, "operation not permitted by the NANO isolate security policy");
  defineCode("ERR_UNSUPPORTED_OPERATION", Error, "operation not supported by the NANO runtime");
  defineCode("ERR_SOCKET_BAD_PORT", RangeError, "Port should be >= 0 and < 65536");
  defineCode("ERR_INVALID_PROTOCOL", TypeError, "invalid protocol");
  defineCode("ERR_INVALID_URL", TypeError, "Invalid URL");
  defineCode("ERR_INVALID_URL_SCHEME", TypeError, "invalid URL scheme");
  defineCode("ERR_INVALID_FILE_URL_PATH", TypeError, "invalid file URL path");
  defineCode("ERR_INVALID_FILE_URL_HOST", TypeError, "invalid file URL host");
  defineCode("ERR_INVALID_THIS", TypeError, "invalid receiver");
  defineCode("ERR_IPC_CHANNEL_CLOSED", Error, "Channel closed");
  defineCode("ERR_ASSERTION", Error, "assertion failed");
  defineCode("ERR_AMBIGUOUS_ARGUMENT", TypeError, "ambiguous argument");
  defineCode("ERR_ENCODING_INVALID_ENCODED_DATA", TypeError, "invalid encoded data");
  defineCode("ERR_HTTP_HEADERS_SENT", Error, "Cannot set headers after they are sent to the client");
  defineCode("ERR_HTTP_INVALID_HEADER_VALUE", TypeError, "Invalid header value");
  defineCode("ERR_HTTP_INVALID_STATUS_CODE", RangeError, "Invalid status code");
  defineCode("ERR_HTTP_TRAILER_INVALID", Error, "Trailers are invalid with this transfer encoding");
  defineCode("ERR_HTTP2_INVALID_SESSION", Error, "The session has been destroyed");
  defineCode("ERR_HTTP2_INVALID_STREAM", Error, "The stream has been destroyed");
  defineCode("ERR_INSPECTOR_NOT_AVAILABLE", Error, "Inspector is not available");
  defineCode("ERR_TRACE_EVENTS_UNAVAILABLE", Error, "Trace events are unavailable");
  defineCode("ERR_WASI_NOT_AVAILABLE", Error, "WASI is not available in the NANO runtime");
  defineCode("ERR_WORKER_NOT_RUNNING", Error, "Worker instance not running");
  defineCode("ERR_DNS_SET_SERVERS_FAILED", Error, "c-ares failed to set servers");
  defineCode("ERR_ZLIB_INITIALIZATION_FAILED", Error, "Initialization failed");
  defineCode("ERR_UNAVAILABLE_DURING_EXIT", Error, "Cannot call function in process exit handler");
  defineCode("ERR_USE_AFTER_CLOSE", Error, "use after close");
  defineCode("ERR_DIR_CLOSED", Error, "Directory handle was closed");
  defineCode("ERR_FS_FILE_TOO_LARGE", RangeError, "File size is greater than 2 GiB");
  defineCode("ERR_FS_EISDIR", Error, "Path is a directory");
  defineCode("ERR_EVENT_RECURSION", Error, "recursive event dispatch");
  defineCode("ERR_FALSY_VALUE_REJECTION", Error, "Promise was rejected with falsy value");
  defineCode("ERR_MULTIPLE_CALLBACK", Error, "Callback called multiple times");
  defineCode("ERR_PROCESS_EXIT", Error, "process.exit() requested request termination");
  defineCode("ERR_SCRIPT_EXECUTION_INTERRUPTED", Error, "Script execution was interrupted");

  function notPermitted(syscall, detail) {
    const Ctor = codes.ERR_OPERATION_NOT_PERMITTED;
    const err = new Ctor(
      (detail ? detail + ": " : "") +
      "'" + syscall + "' is not permitted by the NANO isolate security policy " +
      "(multi-tenant sandbox: no raw sockets, subprocesses, threads, or dynamic code)"
    );
    err.errno = UV_ERRNOS.EPERM;
    err.syscall = syscall;
    return err;
  }

  function unsupported(what) {
    const Ctor = codes.ERR_UNSUPPORTED_OPERATION;
    return new Ctor("'" + what + "' is not supported by the NANO runtime");
  }

  module.exports = {
    codes,
    makeError,
    uvError,
    notPermitted,
    unsupported,
    UV_ERRNOS,
  };
});
