const core = globalThis.Deno.core;
const ops = core.ops;

globalThis.setOutput = function setOutput(name, value) {
    const supportedTypes = [
        "string",
        "boolean",
        "number",
        "undefined",
        "null"
    ];

    if (!supportedTypes.includes(typeof(value))) {
        value = JSON.stringify(value)
    }

    ops.op_set_output(`${name}`, value)
}

globalThis.getTraceHeaders = function getTraceHeaders() {
    const headers = ops.op_get_trace_headers()

    return {
        traceparent: headers.traceparent,
        tracestate: headers.tracestate
    }
}

globalThis.getTraceId = function getTraceId() {
    return ops.op_get_trace_headers().traceparent
}