export class WriteQueue<T extends { rev?: number }> {
  #send: (document: T) => Promise<{ rev: number }>;
  #tail: Promise<unknown> = Promise.resolve();
  #step: { from: number | undefined; to: number } | null = null;
  #generation = 0;
  #waiting = 0;

  constructor(send: (document: T) => Promise<{ rev: number }>) {
    this.#send = send;
  }

  get busy(): boolean {
    return this.#waiting > 0;
  }

  push(document: T): Promise<boolean> {
    const generation = this.#generation;
    this.#waiting += 1;
    const run = this.#tail
      .then(async () => {
        if (generation !== this.#generation) return false;
        const step = this.#step;
        const rebased = step !== null && document.rev === step.from;
        try {
          const answer = await this.#send(rebased ? { ...document, rev: step.to } : document);
          this.#step = { from: rebased ? step.from : document.rev, to: answer.rev };
          return true;
        } catch (cause) {
          this.#generation += 1;
          this.#step = null;
          throw cause;
        }
      })
      .finally(() => {
        this.#waiting -= 1;
      });
    this.#tail = run.catch(() => undefined);
    return run;
  }

  idle(): Promise<void> {
    return this.#tail.then(() => undefined);
  }

  settle(): void {
    if (!this.busy) this.#step = null;
  }
}
