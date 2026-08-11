import { createApi } from "@reduxjs/toolkit/query/react";
import { baseQuery } from "../transport";

export const v1 = createApi({
  reducerPath: "v1",
  baseQuery,
  tagTypes: ["Library", "Media", "Playback"],
  endpoints: () => ({}),
});

export default v1;
