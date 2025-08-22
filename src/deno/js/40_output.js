import { op_set_output, op_get_trace_headers } from "ext:core/ops";

export function setOutput(name, value) {
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

    op_set_output(`${name}`, value)
}

export function getTraceHeaders() {
    const headers = op_get_trace_headers()

    return {
        traceparent: headers.traceparent,
        tracestate: headers.tracestate
    }
}

export function getTraceId() {
    return op_get_trace_headers().traceparent
}

globalThis.setOutput = setOutput;
globalThis.getTraceHeaders = getTraceHeaders;
globalThis.getTraceId = getTraceId;