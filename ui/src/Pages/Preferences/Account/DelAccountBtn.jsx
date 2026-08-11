import { useCallback, useState } from "react";
import { useDispatch } from "react-redux";

import InputConfirmationBox from "../../../Modals/InputConfirmationBox";
import { logout } from "../../../actions/auth.js";
import Button from "../../../Components/Misc/Button";
import { useDeleteAccountMutation } from "../../../api/v1/foundation";
import { addNotification } from "../../../slices/notifications";

function DelAccountBtn() {
  const [deleteAccount] = useDeleteAccountMutation();

  const [pass, setPass] = useState("");
  const [passErr, setPassErr] = useState("");

  const dispatch = useDispatch();

  const confirmDel = useCallback(async () => {
    if (pass.length === 0) {
      setPassErr("Enter your password to continue");
      return false;
    }

    try {
      await deleteAccount({ password: pass }).unwrap();
      dispatch(
        addNotification({
          msg: "Your account has been deleted, you have been logged out.",
        })
      );
      await dispatch(logout());
      window.location.href = "/";
      return true;
    } catch (failure) {
      setPassErr(failure?.message || "Unable to delete the account.");
      return false;
    }
  }, [deleteAccount, dispatch, pass]);

  return (
    <InputConfirmationBox
      title="Confirm action"
      cancelText="Nevermind"
      confirmText="Delete my account"
      action={confirmDel}
      msg="You are about to delete your account, are you sure you want to continue?"
      data={pass}
      setData={setPass}
      err={passErr}
      setErr={setPassErr}
      label="Password"
      type="password"
      icon="key"
    >
      <Button type="critical">
        <p className="logout">Delete account</p>
      </Button>
    </InputConfirmationBox>
  );
}

export default DelAccountBtn;
