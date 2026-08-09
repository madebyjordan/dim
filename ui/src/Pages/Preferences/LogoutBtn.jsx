import { useCallback } from "react";
import { useNavigate } from "react-router";
import { useDispatch } from "react-redux";

import ConfirmationBox from "../../Modals/ConfirmationBox";
import { logout } from "../../actions/auth.js";

function LogoutBtn() {
  const dispatch = useDispatch();
  const navigate = useNavigate();

  const confirmLogout = useCallback(() => {
    dispatch(logout());
    navigate("/login");
  }, [dispatch, navigate]);

  return (
    <ConfirmationBox
      title="Confirm action"
      cancelText="Nevermind"
      confirmText="Yes"
      action={confirmLogout}
      msg="Are you sure you want to logout?"
    >
      <button className="logout">
        <p className="logout">Logout</p>
      </button>
    </ConfirmationBox>
  );
}

export default LogoutBtn;
