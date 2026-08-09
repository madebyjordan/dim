import { useState } from "react";
import "./Toggle.scss";

type ToggleProps = {
  name: string;
  desc?: string | undefined;
  state?: boolean;
  disabled?: boolean;
  onToggle?: (active: boolean) => void;
};

function Toggle(props: ToggleProps) {
  const [uncontrolledActive, setUncontrolledActive] = useState(false);
  const active = props.state ?? uncontrolledActive;

  const toggle = () => {
    if (props.disabled) return;

    const nextActive = !active;
    props.onToggle?.(nextActive);
    if (props.state === undefined) setUncontrolledActive(nextActive);
  };

  return (
    <div className={`toggleContainer disabled-${props.disabled}`}>
      <p>{props.name}</p>
      {props.desc && <p className="desc">{props.desc}</p>}
      <div onClick={toggle} className={`toggle active-${active}`}>
        <div className="ball" />
      </div>
    </div>
  );
}

export default Toggle;
