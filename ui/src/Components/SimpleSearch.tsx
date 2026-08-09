import { useState, type ChangeEvent } from "react";
import SearchIcon from "assets/figma_icons/Search";
import "./SimpleSearch.scss";

export interface SimpleSearchProps {
  placeholder?: string;
  onChange?: (query: string) => void;
}

export const SimpleSearch = ({ placeholder, onChange }: SimpleSearchProps) => {
  const [value, setValue] = useState<string>("");

  const changeValue = (event: ChangeEvent<HTMLInputElement>) => {
    const nextValue = event.target.value;
    setValue(nextValue);
    onChange?.(nextValue);
  };

  return (
    <div className="simple-searchbox">
      <SearchIcon />
      <input
        type="text"
        value={value}
        placeholder={placeholder ? placeholder : "Search files to match"}
        onChange={changeValue}
      />
    </div>
  );
};

export default SimpleSearch;
