import { cloneElement, useCallback, useEffect, useState } from "react";
import Modal from "react-modal";
import { useDispatch, useSelector } from "react-redux";
import { useHistory } from "react-router-dom";

import { newLibrary } from "../../actions/library.js";
import MediaTypeSelection from "./MediaTypeSelection";
import DirSelection from "./DirSelection";
import Field from "../../Pages/Auth/Field";
import Button from "../../Components/Misc/Button";

import "./Index.scss";

Modal.setAppElement("body");

function NewLibraryModal(props) {
  const dispatch = useDispatch();
  const history = useHistory();
  const creating = useSelector((state) => state.library.new_library.creating);
  const [visible, setVisible] = useState(false);

  const [current, setCurrent] = useState(undefined);
  const [name, setName] = useState("");
  const [nameErr, setNameErr] = useState("");
  const [submitErr, setSubmitErr] = useState("");
  const [mediaType, setMediaType] = useState("movie");
  const [selectedFolder, setSelectedFolder] = useState(undefined);

  // prevent scrolling behind Modal
  useEffect(() => {
    visible
      ? (document.body.style.overflow = "hidden")
      : (document.body.style.overflow = "unset");
  }, [visible]);

  const clear = useCallback(() => {
    setName("");
    setCurrent(undefined);
    setSelectedFolder(undefined);
    setMediaType("movie");
    setSubmitErr("");
  }, []);

  const close = useCallback(() => {
    setVisible(false);
    clear();

    if (props.cleanUp) {
      props.cleanUp();
    }
  }, [clear, props]);

  const open = useCallback(() => {
    setVisible(true);
  }, []);

  useEffect(() => {
    if (!name) return;

    const movieRegex = new RegExp("movie|film", "gi");
    const tvShowRegex = new RegExp("tv|show|anime", "gi");
    const matchesMovie = movieRegex.test(name);
    const matchesTvOrShows = tvShowRegex.test(name);

    // TODO: set to 'mixed' when available.
    if (matchesMovie && matchesTvOrShows) {
      return;
    }

    if (matchesMovie) {
      setMediaType("movie");
    }

    if (matchesTvOrShows) {
      setMediaType("tv");
    }
  }, [name]);

  const add = useCallback(async () => {
    if (!name.trim()) {
      setNameErr("Label your library");
      return;
    }

    if (!selectedFolder || creating) return;

    setSubmitErr("");
    const data = {
      name: name.trim(),
      locations: [selectedFolder],
      media_type: mediaType,
    };

    const result = await dispatch(newLibrary(data));
    if (result.ok) {
      close();
      history.push(`/library/${result.id}`);
    } else {
      setSubmitErr(result.error);
    }
  }, [close, creating, dispatch, history, mediaType, name, selectedFolder]);

  return (
    <div className="modalBoxContainer">
      {cloneElement(props.children, { onClick: () => open() })}
      <Modal
        isOpen={visible}
        className="modalBox"
        id="modalNewLibrary"
        onRequestClose={() => !creating && close()}
        overlayClassName="popupOverlay"
      >
        <div className="modalNewLibrary">
          <div className="heading">
            <h3>Create a new library</h3>
            <div className="separator" />
          </div>
          <div className="fields">
            <Field
              name="Name"
              placeholder="Untitled"
              data={[name, setName]}
              error={[nameErr, setNameErr]}
            />
          </div>
          <MediaTypeSelection
            mediaType={mediaType}
            setMediaType={setMediaType}
          />
          <DirSelection
            current={current}
            setCurrent={setCurrent}
            selectedFolder={selectedFolder}
            setSelectedFolder={setSelectedFolder}
          />
          {submitErr && (
            <div className="library-submit-error" role="alert">
              {submitErr}
            </div>
          )}
          <div className="options">
            <Button type="secondary" onClick={close} disabled={creating}>
              Nevermind
            </Button>
            <Button
              disabled={!name.trim() || !selectedFolder || creating}
              onClick={add}
            >
              {creating ? "Creating library…" : "Create Library"}
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}

export default NewLibraryModal;
