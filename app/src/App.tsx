import { useState } from 'react';
import { Editor } from './ui/editor/component/Editor';
import 'bootstrap/dist/css/bootstrap.min.css';

function App() {
  return <div className="container">
    <h1>Shogi Helpmate Solver</h1>
    <Editor />
  </div>;
}

export default App;
