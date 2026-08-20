import { describe, expect, it, vi } from 'vitest';
import { createSeekCommitter } from './seek';

describe('seek committer', () => {
  it('previews an entire pointer scrub but commits only its final target', () => {
    const preview = vi.fn();
    const commit = vi.fn();
    const seek = createSeekCommitter(() => 12, preview, commit);

    seek.preview(30);
    seek.preview(90);
    seek.preview(300);

    expect(commit).not.toHaveBeenCalled();
    expect(seek.isPreviewing()).toBe(true);
    seek.commit(300);
    expect(commit).toHaveBeenCalledOnce();
    expect(commit).toHaveBeenCalledWith({
      from: 12,
      target: 300,
      inputCount: 3
    });
    expect(seek.isPreviewing()).toBe(false);
  });

  it('commits keyboard changes even when no input preview was observed', () => {
    const commit = vi.fn();
    const seek = createSeekCommitter(() => 25, () => undefined, commit);

    seek.commit(35);

    expect(commit).toHaveBeenCalledWith({
      from: 25,
      target: 35,
      inputCount: 1
    });
  });
});
