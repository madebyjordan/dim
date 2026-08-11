import v1 from "./";
import type {
  UnmatchedFiles,
  UnmatchedMediaFile as ContractUnmatchedMediaFile,
} from "../generated";

export type UnmatchedMediaFiles = UnmatchedFiles;
export type UnmatchedMediaFile = ContractUnmatchedMediaFile;

export const media = v1.injectEndpoints({
  endpoints: (build) => ({
    getUnmatchedMediaFiles: build.query<
      UnmatchedMediaFiles,
      { id: string; search?: string | null }
    >({
      query: ({ id, search }) => {
        if (search && search.length > 0)
          return `library/${id}/unmatched?search=${search}`;

        return `library/${id}/unmatched`;
      },
    }),
  }),
});

export const { useGetUnmatchedMediaFilesQuery } = media;

export default media;
