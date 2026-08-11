import v1 from "./";
import type {
  CreateLibrary,
  AdminExists,
  ChangePassword,
  DeleteAccount,
  ExternalMedia,
  Library,
  LoginRequest,
  LoginResponse,
  RegisterResponse,
  PlaybackSession,
  RematchMedia,
  ScanStatus,
} from "../generated";
import type {
  BrowserVideoCapability,
  PlaybackCapabilityInspection,
} from "../../Pages/VideoPlayer/VideoCapabilities";

export const foundation = v1.injectEndpoints({
  endpoints: (build) => ({
    login: build.mutation<LoginResponse, LoginRequest>({
      query: (body) => ({ url: "auth/login", method: "POST", body }),
    }),
    register: build.mutation<RegisterResponse, LoginRequest>({
      query: (body) => ({ url: "auth/register", method: "POST", body }),
    }),
    adminExists: build.query<AdminExists, void>({
      query: () => "auth/admin_exists",
    }),
    changePassword: build.mutation<void, ChangePassword>({
      query: (body) => ({ url: "user/password", method: "PATCH", body }),
    }),
    deleteAccount: build.mutation<void, DeleteAccount>({
      query: (body) => ({ url: "user", method: "DELETE", body }),
    }),
    listLibraries: build.query<Library[], void>({
      query: () => "library",
      providesTags: ["Library"],
    }),
    getLibraryMedia: build.query<Record<string, unknown[]>, string>({
      query: (id) => `library/${id}/media`,
      providesTags: (_result, _error, id) => [{ type: "Library", id }],
    }),
    createLibrary: build.mutation<Library, CreateLibrary>({
      query: (body) => ({ url: "library", method: "POST", body }),
      invalidatesTags: ["Library"],
    }),
    deleteLibrary: build.mutation<void, number>({
      query: (id) => ({ url: `library/${id}`, method: "DELETE" }),
      invalidatesTags: ["Library"],
    }),
    getLibraryScan: build.query<ScanStatus, string>({
      query: (id) => `library/${id}/scan`,
      providesTags: (_result, _error, id) => [{ type: "Library", id }],
    }),
    retryLibraryScan: build.mutation<void, string>({
      query: (id) => ({ url: `library/${id}/scan`, method: "POST" }),
      invalidatesTags: (_result, _error, id) => [{ type: "Library", id }],
    }),
    searchExternalMedia: build.query<
      ExternalMedia[],
      { query: string; mediaType: string }
    >({
      query: ({ query, mediaType }) => ({
        url: "media/tmdb_search",
        params: { query, media_type: mediaType },
      }),
    }),
    rematchMedia: build.mutation<void, { id: string; match: RematchMedia }>({
      query: ({ id, match }) => ({
        url: `media/${id}/rematch`,
        method: "POST",
        body: match,
      }),
      invalidatesTags: (_result, _error, { id }) => [{ type: "Media", id }],
    }),
    saveProgress: build.mutation<void, { id: number; offset: number }>({
      query: ({ id, offset }) => ({
        url: `media/${id}/progress`,
        method: "POST",
        params: { offset },
      }),
      invalidatesTags: (_result, _error, { id }) => [{ type: "Media", id }],
    }),
    inspectPlaybackCapabilities: build.query<
      PlaybackCapabilityInspection,
      string
    >({
      query: (fileId) => ({ url: `stream/${fileId}/capabilities` }),
    }),
    createPlaybackSession: build.mutation<
      PlaybackSession,
      {
        fileId: string;
        forceAss: boolean;
        videoCapability: BrowserVideoCapability | null;
      }
    >({
      query: ({ fileId, forceAss, videoCapability }) => ({
        url: `stream/${fileId}/manifest`,
        params: {
          force_ass: forceAss,
          ...(videoCapability && {
            video_capability: JSON.stringify(videoCapability),
          }),
        },
      }),
      invalidatesTags: ["Playback"],
    }),
    killPlaybackSession: build.mutation<void, string>({
      query: (gid) => ({ url: `stream/${gid}/state/kill`, method: "DELETE" }),
      invalidatesTags: ["Playback"],
    }),
  }),
});

export const {
  useLoginMutation,
  useRegisterMutation,
  useChangePasswordMutation,
  useDeleteAccountMutation,
  useGetLibraryMediaQuery,
  useLazySearchExternalMediaQuery,
  useRematchMediaMutation,
  useSaveProgressMutation,
  useLazyInspectPlaybackCapabilitiesQuery,
  useCreatePlaybackSessionMutation,
  useKillPlaybackSessionMutation,
} = foundation;
