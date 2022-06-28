import { useState } from 'react';
import { Editor as Editor } from './editor/Editor';
import { Solution } from './solution';

function App() {
  const [jkf, setJkf] = useState("");

  return <div>
    <Editor onSolved={jkf => {
      setJkf(jkf);
    }} />
    {jkf ? <Solution jkf={jkf} /> : <></>}
  </div>;
}

export default App;
