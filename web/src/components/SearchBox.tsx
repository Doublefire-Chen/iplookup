import { useId, useState, type FormEvent } from "react";
import { useNavigate } from "react-router";
import { parseIpInput } from "../ip";

/** Opens the Record page of the IP address typed in; anything else gets an inline error. */
export function SearchBox() {
  const navigate = useNavigate();
  const id = useId();
  const [text, setText] = useState("");
  const [error, setError] = useState<string>();

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const ip = parseIpInput(text);
    if (!ip) {
      setError(
        text.trim() === ""
          ? "Type an IP address, such as 8.8.8.8 or 2001:4860:4860::8888."
          : `“${text.trim()}” isn't an IP address. Try one like 8.8.8.8 or 2001:4860:4860::8888.`,
      );
      return;
    }
    setError(undefined);
    setText("");
    // A client-side move: the Record page fetches /{ip}/json itself.
    navigate(`/${ip}`);
  }

  return (
    <form className="search" role="search" onSubmit={submit} noValidate>
      <label htmlFor={`${id}-ip`} className="visually-hidden">
        IP address
      </label>
      <div className="search-row">
        <input
          id={`${id}-ip`}
          type="text"
          autoComplete="off"
          autoCapitalize="off"
          spellCheck={false}
          placeholder="Look up an IP address"
          value={text}
          onChange={(event) => {
            setText(event.target.value);
            setError(undefined);
          }}
          aria-invalid={error !== undefined}
          aria-describedby={error ? `${id}-error` : undefined}
        />
        <button type="submit">Look up</button>
      </div>
      {error && (
        <p id={`${id}-error`} className="search-error" role="alert">
          {error}
        </p>
      )}
    </form>
  );
}
