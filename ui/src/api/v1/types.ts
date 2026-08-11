// This file contains types shared by different parts of the API.

export type { Chapters, Media } from "../generated";

/**
 * A file belonging to one piece of media, such as a movie or an episode of a
 * TV series.
 */
export interface Version {
  display_name: string;
  file: string;
  id: number;
}
