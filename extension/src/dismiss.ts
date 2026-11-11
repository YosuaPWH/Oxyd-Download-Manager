// Videos whose download button the user closed. They stay closed until the page is loaded
// again: feeds redraw their posts all the time, so the choice is remembered by what the
// video is (its post, else its source), not by the element that happened to show it.

/** What identifies a video: `post` is the post's address when the video belongs to one. */
export function dismissalKey(post: string | null | undefined, src: string | null | undefined): string | undefined {
  if (post) return `post:${post}`;
  return src ? `source:${src}` : undefined;
}

export class Dismissals<V extends object> {
  private keys = new Set<string>();
  /** Videos with neither a post nor a source yet: remembered by their element. */
  private bare = new WeakSet<V>();

  add(video: V, key: string | undefined) {
    if (key) this.keys.add(key);
    else this.bare.add(video);
  }

  has(video: V, key: string | undefined): boolean {
    return key ? this.keys.has(key) : this.bare.has(video);
  }
}
