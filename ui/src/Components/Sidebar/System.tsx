import { NavLink } from "react-router";

import WrenchIcon from "../../assets/Icons/Wrench";

const General = () => (
  <section className="yourAccount">
    <header>
      <h4>System</h4>
    </header>
    <div className="list">
      <NavLink
        to="/preferences"
        className={({ isActive }) => `item${isActive ? " active" : ""}`}
      >
        <WrenchIcon />
        <p>Preferences</p>
      </NavLink>
    </div>
  </section>
);

export default General;
