import { useCallback } from "react";
import { useNavigate } from "react-router";
import { useDispatch, useSelector } from "react-redux";

import { delLibrary } from "../../../actions/library";
import ConfirmationBox from "../../../Modals/ConfirmationBox";
import TrashIcon from "../../../assets/Icons/Trash";

const Delete = (props) => {
  const dispatch = useDispatch();

  const del_library = useSelector((store) => store.library.del_library);

  const navigate = useNavigate();

  const removeLib = useCallback(async () => {
    if (del_library.deleting) return;

    dispatch(delLibrary(props.id));

    // redirect to dashboard when removed
    navigate("/");
  }, [del_library.deleting, dispatch, navigate, props.id]);

  const { deleting } = del_library;

  return (
    <div className="deleteLibraryAction">
      <ConfirmationBox
        title="Confirm action"
        cancelText="Nevermind"
        confirmText="Yes"
        action={removeLib}
        msg="Are you sure you want to remove this library?"
      >
        <button className={`delete deleting-${deleting}`}>
          Delete library
          <TrashIcon />
        </button>
      </ConfirmationBox>
    </div>
  );
};

export default Delete;
