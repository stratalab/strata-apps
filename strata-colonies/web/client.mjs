export class ColoniesClient {
  constructor(onEvent = () => {}) {
    this.worker = new Worker(new URL("./worker.mjs", import.meta.url), {
      type: "module",
    });
    this.pending = new Map();
    this.nextId = 1;
    this.closed = false;
    this.worker.onmessage = ({ data }) => {
      if (data.event) {
        onEvent(data);
        return;
      }
      const pending = this.pending.get(data.id);
      if (!pending) return;
      this.pending.delete(data.id);
      if (data.error) pending.reject(new Error(data.error));
      else pending.resolve(data);
    };
    this.worker.onerror = (event) => {
      event.preventDefault();
      const error = new Error(
        event.message ||
          "The browser engine could not load. Check your connection and try again.",
      );
      this.terminate(error);
      onEvent({ event: "error", error: error.message });
    };
  }
  request(type, args) {
    if (this.closed)
      return Promise.reject(new Error("The browser session is closed."));
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.worker.postMessage({ id, type, args });
    });
  }
  terminate(error = new Error("The browser session is closed.")) {
    this.closed = true;
    this.worker.terminate();
    for (const request of this.pending.values()) request.reject(error);
    this.pending.clear();
  }
}
