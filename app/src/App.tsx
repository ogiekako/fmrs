import { useState } from 'react';
import { Editor } from './ui/editor/component/Editor';
import { Solution } from './ui/solution';
import 'bootstrap/dist/css/bootstrap.min.css';

function App() {
  const [jkf, setJkf] = useState("");

  return <div className="container">
    <h1>Shogi Help Mate Solver</h1>
    <Editor onSolved={jkf => {
      setJkf(jkf);
    }} />
    {jkf ? <Solution jkf={jkf} /> : <></>}
  </div>;
}

export default App;
