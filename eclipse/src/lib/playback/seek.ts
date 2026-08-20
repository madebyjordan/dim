export interface CommittedSeek {
  from: number;
  target: number;
  inputCount: number;
}

export function createSeekCommitter(
  currentTime: () => number,
  preview: (target: number) => void,
  commit: (seek: CommittedSeek) => void
) {
  let from: number | null = null;
  let inputCount = 0;

  return {
    preview(target: number) {
      from ??= currentTime();
      inputCount += 1;
      preview(target);
    },
    commit(target: number) {
      const seek = {
        from: from ?? currentTime(),
        target,
        inputCount: Math.max(inputCount, 1)
      };
      from = null;
      inputCount = 0;
      commit(seek);
    },
    isPreviewing() {
      return from !== null;
    }
  };
}
