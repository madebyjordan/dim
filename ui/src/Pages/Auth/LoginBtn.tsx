import React, { useCallback, useEffect } from "react";

import { useAppDispatch } from "hooks/store";
import { AUTH_LOGIN_OK } from "actions/types.js";
import { useLoginMutation } from "api/v1/foundation";
import type { ClientError } from "api/transport";

interface Props {
  credentials: [string, string];
  error: [
    React.Dispatch<React.SetStateAction<string>>,
    React.Dispatch<React.SetStateAction<string>>
  ];
}

function LoginBtn(props: Props) {
  const dispatch = useAppDispatch();
  const [login, { isLoading }] = useLoginMutation();

  const { credentials, error } = props;

  const [username, password] = credentials;
  const [setUsernameErr, setPasswordErr] = error;

  const authorize = useCallback(async () => {
    if (isLoading) return;

    const allowedChars = /^[a-zA-Z0-9_.-]*$/;

    const usernameValidChars = allowedChars.test(username);
    const usernameValidLength = username.length >= 3 && username.length <= 30;

    if (!usernameValidLength) {
      setUsernameErr("Minimum 3 and maximum 30 characters");
      return;
    }

    if (!usernameValidChars) {
      setUsernameErr("Only allowed underscores, dashes or dots");
      return;
    }

    if (password.length < 8) {
      setPasswordErr("Minimum 8 characters");
      return;
    }

    try {
      const payload = await login({ username, password }).unwrap();
      if ("BroadcastChannel" in window) {
        const channel = new BroadcastChannel("dim");
        channel.postMessage("login");
        channel.close();
      }
      dispatch({ type: AUTH_LOGIN_OK, payload });
    } catch (error) {
      setPasswordErr((error as ClientError).message ?? "Unable to sign in.");
    }
  }, [
    dispatch,
    isLoading,
    login,
    password,
    setPasswordErr,
    setUsernameErr,
    username,
  ]);

  const onKeyDown = useCallback(
    (e: KeyboardEvent) => {
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
      Login
    </button>
  );
}

export default LoginBtn;
