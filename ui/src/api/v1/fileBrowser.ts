import v1 from "./";

export interface DirectoryEntry {
  name: string;
  path: string;
}

export interface DirectoryListing {
  current: string;
  parent: string | null;
  directories: DirectoryEntry[];
}

export const fileBrowser = v1.injectEndpoints({
  endpoints: (build) => ({
    getDirectories: build.query<DirectoryListing, string | undefined>({
      query: (path) => ({
        url: "filebrowser",
        params: path ? { path } : undefined,
      }),
    }),
  }),
});

export const { useGetDirectoriesQuery } = fileBrowser;

export default fileBrowser;
