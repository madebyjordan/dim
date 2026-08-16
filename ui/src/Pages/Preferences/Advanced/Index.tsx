import General from "./General";
import DirectoryPaths from "./DirectoryPaths";
import { useSelector } from "react-redux";

import "./Index.scss";

const PreferencesAdvanced = () => {
  const restartRequired = useSelector(
    (store: any) => store.settings.globalSettings.data.restart_required
  );
  return (
    <div className="preferencesAdvanced">
      {restartRequired && (
        <p role="status">
          Host settings are saved but will take effect after Dim restarts.
        </p>
      )}
      <General />
      <DirectoryPaths />
    </div>
  );
};

export default PreferencesAdvanced;
