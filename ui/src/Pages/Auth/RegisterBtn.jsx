import { useCallback, useEffect } from "react";
import { useDispatch, useSelector } from "react-redux";
import { AUTH_LOGIN_OK } from "../../actions/types.js";
import { useRegisterMutation } from "../../api/v1/foundation";

function RegisterBtn(props) {
  const dispatch = useDispatch();
  const [register, { isLoading }] = useRegisterMutation();

  const admin_exists = useSelector((store) => store.auth.admin_exists);

  const { credentials, error, registering } = props;

  const [username, pass, invite] = credentials;
  const [setUsernameErr, setPassErr, setInviteErr] = error;

  const authorize = useCallback(async () => {
    if (isLoading || registering) return;

    const allowedChars = /^[a-zA-Z0-9_.-]*$/;

    const usernameValidChars = allowedChars.test(username);
    const usernameValidLength = username.length >= 3 && username.length <= 30;

    if (!usernameValidLength) {
      setUsernameErr("Minimum 3 and maximum 30 characters");
      return;
    }

    if (!usernameValidChars) {
      setUsernameErr("Only allowed underscores, dashes or dots.");
      return;
    }

    if (pass.length < 8) {
      setPassErr("Minimum 8 characters.");
      return;
    }

    if (admin_exists) {
      if (invite.length !== 36) {
        setInviteErr("Code has to be 36 characters.");
        return;
      }
    }

    try {
      const payload = await register({
        username,
        password: pass,
        ...(admin_exists && { invite_token: invite }),
      }).unwrap();
      dispatch({ type: AUTH_LOGIN_OK, payload });
      if ("BroadcastChannel" in window) {
        const channel = new BroadcastChannel("dim");
        channel.postMessage("login");
        channel.close();
      }
    } catch (failure) {
      setInviteErr(failure?.message || "Unable to register.");
    }
  }, [
    admin_exists,
    dispatch,
    invite,
    isLoading,
    pass,
    registering,
    register,
    setInviteErr,
    setPassErr,
    setUsernameErr,
    username,
  ]);

  const onKeyDown = useCallback(
    (e) => {
      if (e.keyCode === 13) {
        authorize();
      }
    },
    [authorize]
  );

  useEffect(() => {
    window.addEventListener("keydown", onKeyDown);

    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [onKeyDown]);

  return (
    <button className={`${isLoading}`} onClick={authorize} disabled={isLoading}>
      Register
    </button>
  );
}

export default RegisterBtn;
