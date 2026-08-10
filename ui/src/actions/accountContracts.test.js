import { changePassword, createNewInvite, delAccount } from "./auth";
import { changeUsername, delAvatar } from "./user";
import { buildRematchRequest } from "../Modals/RematchMedia/Index";

const state = () => ({ auth: { token: "token" } });

describe("account API contracts", () => {
  afterEach(() => vi.restoreAllMocks());

  it.each([
    [changePassword("old", "new"), "/api/v1/user/password", "PATCH"],
    [delAccount("password"), "/api/v1/user", "DELETE"],
    [createNewInvite(), "/api/v1/auth/invites", "POST"],
    [
      changeUsername({ info: {} }, "new-name"),
      "/api/v1/user/username",
      "PATCH",
    ],
    [delAvatar(), "/api/v1/user/avatar", "DELETE"],
  ])("calls %s with the agreed method", async (action, path, method) => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      status: 200,
      json: async () => ({ token: "invite" }),
    });

    await action(vi.fn(), state);

    expect(fetch).toHaveBeenCalledWith(
      path,
      expect.objectContaining({ method })
    );
  });

  it("sends media rematches as JSON to the rematch endpoint", () => {
    expect(buildRematchRequest(42, "token", 123, "movie")).toEqual({
      url: "/api/v1/media/42/rematch",
      config: {
        method: "POST",
        headers: {
          authorization: "token",
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ external_id: "123", media_type: "movie" }),
      },
    });
  });
});
