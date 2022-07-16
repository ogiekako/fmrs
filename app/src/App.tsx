import { useState } from 'react';
import { Editor as Editor } from './ui/editor/Editor';
import { Solution } from './ui/solution';

function App() {
  const [jkf, setJkf] = useState("");

  return <div>
    <Editor onSolved={jkf => {
      console.log(`solved!! `, jkf);
      setJkf(jkf);
    }} />
    {jkf ? <Solution jkf={jkf} /> : <></>}
  </div>;
}

export default App;
